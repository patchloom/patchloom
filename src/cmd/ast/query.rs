//! Read-only `patchloom ast` subcommands (list, read, validate, search, refs, deps, map, impact, diff).
//! size-waiver: accepted single-domain bulk (policy #1408). Query CLI plus
//! parse-timeout fail-closed locks live in one command module.

use super::common::{
    collect_source_files, display_path, filter_symbols, get_git_file_content, parse_kind_filter,
    print_symbol_items_json, print_symbols_compact, print_symbols_human, print_symbols_json,
    resolve_lang, resolve_target_paths, setup_multi_file, setup_single_file, symbol_to_json,
};
use crate::ast::parse_lang_hint;
use crate::ast::symbols::{self, SymbolDef};
use crate::cli::global::GlobalFlags;
use crate::exit;
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

    let kind_filter = parse_kind_filter(&args.kind)?;
    let lang_hint = args.lang.as_deref().map(parse_lang_hint).transpose()?;
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
        // Strict sole-path (#1894): binary / invalid UTF-8 before language miss.
        let source = match crate::files::load_text_strict(&target, &args.path) {
            Ok(s) => s,
            Err(e) if crate::exit::is_load_text_strict_fail(&e) => {
                let kind = crate::fallback::error_kind_str(&e).unwrap_or("invalid_input");
                let msg = crate::exit::agent_error_message(&e);
                global.emit_error_json_kind(Some(kind), &msg)?;
                return Ok(exit::FAILURE);
            }
            Err(e) => return Err(e),
        };
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
            // invalid_input: not an empty symbol set (agents must not widen search).
            global.emit_error_json_kind(Some("invalid_input"), &msg)?;
            return Ok(exit::FAILURE);
        }
        let symbols = match symbols::try_extract_symbols(&source, lang) {
            Ok(s) => s,
            Err(crate::ast::ParseFailure::DeadlineExceeded) => {
                return Err(crate::exit::ParseTimeoutError {
                    msg: format!("parse deadline exceeded for {}", args.path),
                }
                .into());
            }
            Err(crate::ast::ParseFailure::NoGrammar) => Vec::new(),
        };
        let filtered = filter_symbols(&symbols, &kind_filter);
        if !filtered.is_empty() {
            any_output = true;
            if structured {
                print_symbols_json(&args.path, &filtered, global)?;
            } else if !global.quiet && args.compact {
                print_symbols_compact(&args.path, &filtered);
            } else if !global.quiet {
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

        let timeout: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
        let results: Vec<ListFileResult> =
            crate::par_process_files(&paths, glob_matcher.as_ref(), &glob_roots, |path| {
                let lang = resolve_lang(lang_hint, path);
                let symbols = match symbols::try_extract_symbols_from_file(path, Some(lang)) {
                    Ok(s) => s,
                    Err(crate::ast::ParseFailure::DeadlineExceeded) => {
                        let mut slot = timeout.lock().unwrap_or_else(|e| e.into_inner());
                        if slot.is_none() {
                            *slot = Some(path.display().to_string());
                        }
                        return None;
                    }
                    Err(crate::ast::ParseFailure::NoGrammar) => Vec::new(),
                };
                if symbols.is_empty() {
                    return None;
                }
                let display = display_path(path, &cwd);
                Some(ListFileResult { display, symbols })
            });
        if let Some(file) = timeout.into_inner().unwrap_or_else(|e| e.into_inner()) {
            return Err(crate::exit::ParseTimeoutError {
                msg: format!("parse deadline exceeded for {file}"),
            }
            .into());
        }

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
            } else if !global.quiet && args.compact {
                print_symbols_compact(&result.display, &filtered);
            } else if !global.quiet {
                print_symbols_human(&result.display, &filtered);
            }
        }
        if structured && !structured_items.is_empty() {
            print_symbol_items_json(&structured_items, global)?;
        }
        if !any_output {
            // Unreadable files soft-skip to empty; do not claim "no symbols".
            if let Some(err) = crate::ops::file::empty_scan_masked_by_unreadable(&paths, &cwd) {
                global.emit_error_json_kind(Some("invalid_input"), &err.msg)?;
                return Ok(exit::FAILURE);
            }
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
    // Unsupported language is invalid_input (list/validate parity), not
    // "symbol not found" which sends agents hunting alternate spellings.
    if !lang.has_grammar() {
        let msg = format!(
            "Unsupported language: {} (detected from {}). \
             Supported: Rust, Python, TypeScript, JavaScript, Go, Java, \
             C#, Ruby, PHP, Swift, Kotlin, C, C++, HCL, XML, Protobuf, \
             TOML, YAML, JSON, Shell.",
            lang, args.path,
        );
        global.emit_error_json_kind(Some("invalid_input"), &msg)?;
        return Ok(exit::FAILURE);
    }
    let all_symbols = match symbols::try_extract_symbols(&source, lang) {
        Ok(s) => s,
        Err(crate::ast::ParseFailure::DeadlineExceeded) => {
            return Err(crate::exit::ParseTimeoutError {
                msg: format!("parse deadline exceeded for {}", args.path),
            }
            .into());
        }
        Err(crate::ast::ParseFailure::NoGrammar) => Vec::new(),
    };
    let sym = match symbols::find_symbol(&all_symbols, &args.symbol) {
        Some(s) => s,
        None => {
            let msg = format!("symbol '{}' not found in {}", args.symbol, args.path);
            global.emit_error_json_kind(Some("no_matches"), &msg)?;
            return Ok(exit::NO_MATCHES);
        }
    };

    let lines: Vec<&str> = crate::ops::file::text_lines(&source).collect();
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
    }))? && !global.quiet
    {
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
    let lang_hint = args.lang.as_deref().map(parse_lang_hint).transpose()?;
    crate::verbose!("ast validate: target={}", args.path);

    // Empty directory / no grammar files: fail closed (not vacuous success).
    if paths.is_empty() {
        let msg = format!("no source files to validate in {}", args.path);
        global.emit_error_json_kind(Some("no_matches"), &msg)?;
        return Ok(exit::NO_MATCHES);
    }

    if let Err(err) = super::common::reject_sole_explicit_non_text(&paths, &args.path) {
        let kind = crate::fallback::error_kind_str(&err).unwrap_or("invalid_input");
        let msg = crate::exit::agent_error_message(&err);
        global.emit_error_json_kind(Some(kind), &msg)?;
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
            // invalid_input: not an empty symbol set (agents must not widen search).
            global.emit_error_json_kind(Some("invalid_input"), &msg)?;
            return Ok(exit::FAILURE);
        }
    }

    let mut all_valid = true;
    crate::verbose!("ast validate: checking {} files", paths.len());

    // Preflight grammar paths: any binary/unreadable co-path fails closed.
    // Soft-dropping mid-walk would report partial trees as validated OK.
    for path in &paths {
        let lang = resolve_lang(lang_hint, path);
        if !lang.has_grammar() {
            continue;
        }
        let display = display_path(path, &cwd);
        if let Err(e) = crate::files::load_text_strict(path, &display)
            && crate::exit::is_load_text_strict_fail(&e)
        {
            let kind = crate::fallback::error_kind_str(&e).unwrap_or("invalid_input");
            let msg = crate::exit::agent_error_message(&e);
            global.emit_error_json_kind(Some(kind), &msg)?;
            return Ok(exit::FAILURE);
        }
    }

    struct ValidateFileResult {
        display: String,
        result: crate::ast::validate::ValidationResult,
    }

    let results: Vec<ValidateFileResult> = if paths.len() == 1 {
        // Sole explicit path: surface parse_timeout instead of walk-soft invalid.
        let path = &paths[0];
        let lang = resolve_lang(lang_hint, path);
        match crate::ast::validate::validate_file(path, Some(lang)) {
            Ok(result) => vec![ValidateFileResult {
                display: display_path(path, &cwd),
                result,
            }],
            Err(e) if crate::exit::is_parse_timeout(&e) => {
                return Err(e);
            }
            Err(e) => return Err(e),
        }
    } else {
        let glob_matcher = crate::build_glob_matcher_from_global(global)?;
        let glob_roots = vec![cwd.join(&args.path)];
        crate::par_process_files(&paths, glob_matcher.as_ref(), &glob_roots, |path| {
            let lang = resolve_lang(lang_hint, path);
            if !lang.has_grammar() {
                return None;
            }
            let result = crate::ast::validate::validate_file_for_walk(path, Some(lang))?;
            let display = display_path(path, &cwd);
            Some(ValidateFileResult { display, result })
        })
    };

    // Unreadable co-paths must not look like clean validate or empty grammar set.
    if let Some(err) = crate::ops::file::empty_scan_masked_by_unreadable(&paths, &cwd) {
        global.emit_error_json_kind(Some("invalid_input"), &err.msg)?;
        return Ok(exit::FAILURE);
    }

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
        // JSONL file rows have no top-level error_kind. Tidy check streams a
        // summary trailer so agents can branch; --json stays a bare array.
        if global.jsonl {
            let invalid_count = results.iter().filter(|vr| !vr.result.valid).count();
            global.emit_json(&serde_json::json!({
                "type": "summary",
                "ok": all_valid,
                "file_count": results.len(),
                "invalid_count": invalid_count,
                "error_kind": if all_valid {
                    None
                } else {
                    Some("validation_failed")
                },
            }))?;
        }
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

    /// Treat the query as a code pattern with `$VAR` meta-variables.
    /// The pattern must be valid source after substituting `$VAR` (use
    /// `fn $NAME() {}`, not `fn $NAME()`). Literal tokens match exactly.
    /// `$$$MULTI` is not implemented (use an S-expression query instead).
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
    if let Err(err) = super::common::reject_sole_explicit_non_text(&paths, &args.path) {
        let kind = crate::fallback::error_kind_str(&err).unwrap_or("invalid_input");
        let msg = crate::exit::agent_error_message(&err);
        global.emit_error_json_kind(Some(kind), &msg)?;
        return Ok(exit::FAILURE);
    }
    let lang_hint = args.lang.as_deref().map(parse_lang_hint).transpose()?;
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

    // Fail closed on invalid S-expression / pattern compile before the walk
    // soft-drops ParseError into no_matches.
    if let Some(sample) = paths
        .iter()
        .find(|p| resolve_lang(lang_hint, p).has_grammar())
    {
        let lang = resolve_lang(lang_hint, sample);
        let query_str = if args.pattern {
            match crate::ast::search::compile_pattern_query(&args.query, lang) {
                Ok(q) => q,
                Err(e) => {
                    let kind = crate::fallback::error_kind_str(&e).unwrap_or("parse_error");
                    let msg = crate::exit::agent_error_message(&e);
                    global.emit_error_json_kind(Some(kind), &msg)?;
                    return Ok(exit::PARSE_ERROR);
                }
            }
        } else {
            args.query.clone()
        };
        if let Err(e) = crate::ast::search::search_file(sample, &query_str, Some(lang), Some(1)) {
            if crate::exit::is_parse_timeout(&e) {
                return Err(e);
            }
            if crate::exit::is_parse_error(&e) {
                let msg = crate::exit::agent_error_message(&e);
                global.emit_error_json_kind(Some("parse_error"), &msg)?;
                return Ok(exit::PARSE_ERROR);
            }
        }
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
                        if !global.quiet && !global.json && !global.jsonl {
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
            } else if !global.quiet {
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
        // Unreadable-masked walks must not look like pattern miss.
        if let Some(err) = crate::ops::file::empty_scan_masked_by_unreadable(&paths, &cwd) {
            global.emit_error_json_kind(Some("invalid_input"), &err.msg)?;
            return Ok(exit::FAILURE);
        }
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
    if let Err(err) = super::common::reject_sole_explicit_non_text(&paths, &args.path) {
        let kind = crate::fallback::error_kind_str(&err).unwrap_or("invalid_input");
        let msg = crate::exit::agent_error_message(&err);
        global.emit_error_json_kind(Some(kind), &msg)?;
        return Ok(exit::FAILURE);
    }
    let lang_hint = args.lang.as_deref().map(parse_lang_hint).transpose()?;
    crate::verbose!("ast refs: symbol={}, target={}", args.symbol, args.path);
    crate::verbose!("ast refs: scanning {} files", paths.len());

    let mut all_refs = if paths.len() == 1 {
        let path = &paths[0];
        let display = display_path(path, &cwd);
        let lang = resolve_lang(lang_hint, path);
        let source = crate::files::load_text_strict(path, &args.path)?;
        match crate::ast::refs::try_find_refs_in_source(&source, &args.symbol, lang, &display) {
            Ok(refs) => refs,
            Err(crate::ast::ParseFailure::DeadlineExceeded) => {
                return Err(crate::exit::ParseTimeoutError {
                    msg: format!("parse deadline exceeded for {}", args.path),
                }
                .into());
            }
            Err(crate::ast::ParseFailure::NoGrammar) => Vec::new(),
        }
    } else {
        let glob_matcher = crate::build_glob_matcher_from_global(global)?;
        let glob_roots = vec![cwd.join(&args.path)];
        let timeout: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
        let per_file_refs: Vec<Vec<crate::ast::refs::SymbolRef>> =
            crate::par_process_files(&paths, glob_matcher.as_ref(), &glob_roots, |path| {
                let display = display_path(path, &cwd);
                let refs = match crate::ast::refs::try_find_refs_in_file(
                    path,
                    &args.symbol,
                    lang_hint,
                    &display,
                ) {
                    Ok(refs) => refs,
                    Err(crate::ast::ParseFailure::DeadlineExceeded) => {
                        let mut slot = timeout.lock().unwrap_or_else(|e| e.into_inner());
                        if slot.is_none() {
                            *slot = Some(path.display().to_string());
                        }
                        return None;
                    }
                    Err(crate::ast::ParseFailure::NoGrammar) => Vec::new(),
                };
                if refs.is_empty() { None } else { Some(refs) }
            });
        if let Some(file) = timeout.into_inner().unwrap_or_else(|e| e.into_inner()) {
            return Err(crate::exit::ParseTimeoutError {
                msg: format!("parse deadline exceeded for {file}"),
            }
            .into());
        }
        per_file_refs.into_iter().flatten().collect()
    };

    if !args.include_def {
        all_refs.retain(|r| r.kind != crate::ast::refs::RefKind::Definition);
    }

    if all_refs.is_empty() {
        if let Some(err) = crate::ops::file::empty_scan_masked_by_unreadable(&paths, &cwd) {
            global.emit_error_json_kind(Some("invalid_input"), &err.msg)?;
            return Ok(exit::FAILURE);
        }
        let msg = format!("no references found for '{}' in {}", args.symbol, args.path);
        global.emit_error_json_kind(Some("no_matches"), &msg)?;
        return Ok(exit::NO_MATCHES);
    }

    if !global.emit_json(&serde_json::json!({
        "symbol": args.symbol,
        "references": all_refs,
        "count": all_refs.len(),
    }))? && !global.quiet
    {
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
    let lang_hint = args.lang.as_deref().map(parse_lang_hint).transpose()?;
    crate::verbose!("ast deps: target={}, reverse={}", args.path, args.reverse);

    let paths = resolve_target_paths(&target, &args.path, global)?;
    crate::verbose!("ast deps: scanning {} files", paths.len());

    if let Err(err) = super::common::reject_sole_explicit_non_text(&paths, &args.path) {
        let kind = crate::fallback::error_kind_str(&err).unwrap_or("invalid_input");
        let msg = crate::exit::agent_error_message(&err);
        global.emit_error_json_kind(Some(kind), &msg)?;
        return Ok(exit::FAILURE);
    }

    let mut any_output = false;
    let structured = global.json || global.jsonl;
    let mut structured_items: Vec<serde_json::Value> = Vec::new();
    // Reverse deps scan cwd; empty-mask must use that scan set, not only `paths`.
    let mut reverse_scan_files: Option<Vec<std::path::PathBuf>> = None;

    if !args.reverse && paths.len() == 1 {
        let path = &paths[0];
        let lang = resolve_lang(lang_hint, path);
        let source = crate::files::load_text_strict(path, &args.path)?;
        let imports = match crate::ast::deps::try_extract_imports(&source, lang) {
            Ok(i) => i,
            Err(crate::ast::ParseFailure::DeadlineExceeded) => {
                return Err(crate::exit::ParseTimeoutError {
                    msg: format!("parse deadline exceeded for {}", args.path),
                }
                .into());
            }
            Err(crate::ast::ParseFailure::NoGrammar) => Vec::new(),
        };
        if imports.is_empty() {
            let msg = format!("no imports found in {}", args.path);
            global.emit_error_json_kind(Some("no_matches"), &msg)?;
            return Ok(exit::NO_MATCHES);
        }
        let display = display_path(path, &cwd);
        if structured {
            structured_items.push(serde_json::json!({
                "file": display,
                "imports": imports,
            }));
            global.emit_json_items(&structured_items)?;
        } else if !global.quiet {
            println!("{display}");
            println!("  imports:");
            for imp in &imports {
                println!("    {}", imp.path);
            }
            println!();
        }
        return Ok(exit::SUCCESS);
    }

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
        reverse_scan_files = Some(all_files.clone());

        struct ReverseHit {
            display: String,
            matching: Vec<crate::ast::deps::Import>,
        }

        let timeout: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
        let hits: Vec<ReverseHit> = crate::par_process_files(&all_files, None, &[], |path| {
            let imports = match crate::ast::deps::try_extract_imports_from_file(path, lang_hint) {
                Ok(i) => i,
                Err(crate::ast::ParseFailure::DeadlineExceeded) => {
                    let mut slot = timeout.lock().unwrap_or_else(|e| e.into_inner());
                    if slot.is_none() {
                        *slot = Some(path.display().to_string());
                    }
                    return None;
                }
                Err(crate::ast::ParseFailure::NoGrammar) => Vec::new(),
            };
            // Use segment-boundary matching to avoid substring false positives.
            // Split import paths on common separators (::, /, .) and check if
            // any segment exactly equals the target file stem.
            let matching: Vec<_> = imports
                .into_iter()
                .filter(|i| crate::ast::deps::import_path_refers_to_stem(&i.path, &target_name))
                .collect();
            if matching.is_empty() {
                return None;
            }
            Some(ReverseHit {
                display: display_path(path, &cwd),
                matching,
            })
        });
        if let Some(file) = timeout.into_inner().unwrap_or_else(|e| e.into_inner()) {
            return Err(crate::exit::ParseTimeoutError {
                msg: format!("parse deadline exceeded for {file}"),
            }
            .into());
        }

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
            } else if !global.quiet {
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
        let timeout: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
        let results: Vec<DepsFileResult> =
            crate::par_process_files(&paths, glob_matcher.as_ref(), &glob_roots, |path| {
                let imports = match crate::ast::deps::try_extract_imports_from_file(path, lang_hint)
                {
                    Ok(i) => i,
                    Err(crate::ast::ParseFailure::DeadlineExceeded) => {
                        let mut slot = timeout.lock().unwrap_or_else(|e| e.into_inner());
                        if slot.is_none() {
                            *slot = Some(path.display().to_string());
                        }
                        return None;
                    }
                    Err(crate::ast::ParseFailure::NoGrammar) => Vec::new(),
                };
                if imports.is_empty() {
                    return None;
                }
                let display = display_path(path, &cwd);
                Some(DepsFileResult { display, imports })
            });
        if let Some(file) = timeout.into_inner().unwrap_or_else(|e| e.into_inner()) {
            return Err(crate::exit::ParseTimeoutError {
                msg: format!("parse deadline exceeded for {file}"),
            }
            .into());
        }

        for result in &results {
            any_output = true;
            if structured {
                structured_items.push(serde_json::json!({
                    "file": result.display,
                    "imports": result.imports,
                }));
            } else if !global.quiet {
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
        let mask_paths = reverse_scan_files.as_deref().unwrap_or(&paths);
        if let Some(err) = crate::ops::file::empty_scan_masked_by_unreadable(mask_paths, &cwd) {
            global.emit_error_json_kind(Some("invalid_input"), &err.msg)?;
            return Ok(exit::FAILURE);
        }
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

    let entries = crate::ast::map::try_generate_map(&file_pairs, &opts)?;

    if entries.is_empty() {
        if let Some(err) = crate::ops::file::empty_scan_masked_by_unreadable(&paths, &cwd) {
            global.emit_error_json_kind(Some("invalid_input"), &err.msg)?;
            return Ok(exit::FAILURE);
        }
        let msg = format!("no symbols found in {}", args.path);
        global.emit_error_json_kind(Some("no_matches"), &msg)?;
        return Ok(exit::NO_MATCHES);
    }

    if !global.emit_json_items(&entries)? && !global.quiet {
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
    if let Err(err) = super::common::reject_sole_explicit_non_text(&paths, &args.path) {
        let kind = crate::fallback::error_kind_str(&err).unwrap_or("invalid_input");
        let msg = crate::exit::agent_error_message(&err);
        global.emit_error_json_kind(Some(kind), &msg)?;
        return Ok(exit::FAILURE);
    }
    let _lang_hint = args.lang.as_deref().map(parse_lang_hint).transpose()?;
    crate::verbose!("ast impact: symbol={}, depth={}", args.symbol, args.depth);
    crate::verbose!("ast impact: scanning {} files", paths.len());

    let file_pairs: Vec<(std::path::PathBuf, String)> = paths
        .iter()
        .map(|p| {
            let display = display_path(p, &cwd);
            (p.clone(), display)
        })
        .collect();

    let nodes = match crate::ast::impact::try_compute_impact(&args.symbol, &file_pairs, args.depth)
    {
        Ok(n) => n,
        Err(crate::ast::ParseFailure::DeadlineExceeded) => {
            return Err(crate::exit::ParseTimeoutError {
                msg: format!("parse deadline exceeded for {}", args.path),
            }
            .into());
        }
        Err(crate::ast::ParseFailure::NoGrammar) => Vec::new(),
    };

    if nodes.is_empty() {
        if let Some(err) = crate::ops::file::empty_scan_masked_by_unreadable(&paths, &cwd) {
            global.emit_error_json_kind(Some("invalid_input"), &err.msg)?;
            return Ok(exit::FAILURE);
        }
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
    }))? && !global.quiet
    {
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
    let lang = resolve_lang(
        args.lang.as_deref().map(parse_lang_hint).transpose()?,
        &target,
    );
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
        // Strict sole-path working tree (#1894).
        crate::files::load_text_strict(&target, &args.path)?
    };

    let changes = match crate::ast::diff::try_structural_diff(&old_source, &new_source, lang) {
        Ok(c) => c,
        Err(crate::ast::ParseFailure::DeadlineExceeded) => {
            return Err(crate::exit::ParseTimeoutError {
                msg: format!("parse deadline exceeded for {}", args.path),
            }
            .into());
        }
        Err(crate::ast::ParseFailure::NoGrammar) => Vec::new(),
    };

    if changes.is_empty() {
        global.emit_error_json_kind(Some("no_matches"), "no structural changes")?;
        return Ok(exit::NO_MATCHES);
    }

    if !global.emit_json(&serde_json::json!({
        "file": args.path,
        "from": args.from,
        "to": args.to.as_deref().unwrap_or("working tree"),
        "changes": changes,
    }))? && !global.quiet
    {
        print!("{}", crate::ast::diff::render_changes(&args.path, &changes));
    }

    Ok(exit::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::global::GlobalFlags;
    use std::fs;
    use tempfile::TempDir;

    fn nested_rust_source(depth: usize) -> String {
        let mut source = String::from("fn main() { let x = ");
        source.push_str(&"(".repeat(depth));
        source.push('1');
        source.push_str(&")".repeat(depth));
        source.push_str("; }\n");
        source
    }

    fn assert_search_timeout(result: anyhow::Result<u8>) {
        match result {
            Ok(code) => {
                panic!("sole-file timeout must return Err(ParseTimeoutError), got Ok({code})")
            }
            Err(e) => assert!(
                crate::exit::is_parse_timeout(&e),
                "expected parse_timeout, got {e}"
            ),
        }
    }

    #[test]
    fn search_sole_file_timeout_is_parse_timeout() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("deep.rs"), nested_rust_source(80_000)).unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let result = run_search(
            SearchArgs {
                query: "(function_item) @fn".into(),
                path: "deep.rs".into(),
                pattern: false,
                lang: None,
                max_results: None,
            },
            &global,
        );
        assert_search_timeout(result);
    }

    #[test]
    fn search_sole_file_pattern_timeout_is_parse_timeout() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("deep.rs"), nested_rust_source(80_000)).unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let result = run_search(
            SearchArgs {
                query: "fn $NAME() {}".into(),
                path: "deep.rs".into(),
                pattern: true,
                lang: Some("rs".into()),
                max_results: None,
            },
            &global,
        );
        assert_search_timeout(result);
    }

    #[test]
    fn validate_sole_file_timeout_is_parse_timeout() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("deep.rs"), nested_rust_source(80_000)).unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let result = run_validate(
            ValidateArgs {
                path: "deep.rs".into(),
                lang: None,
            },
            &global,
        );
        match result {
            Ok(code) => panic!(
                "sole-path validate timeout must return Err(ParseTimeoutError), got Ok({code})"
            ),
            Err(e) => assert!(
                crate::exit::is_parse_timeout(&e),
                "expected parse_timeout, got {e}"
            ),
        }
    }

    #[test]
    fn list_unknown_lang_hint_is_invalid_input() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("mod.py"), "def x():\n    pass\n").unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());
        let err = run_list(
            ListArgs {
                path: "mod.py".into(),
                kind: None,
                compact: false,
                lang: Some("python3".into()),
            },
            &global,
        )
        .unwrap_err();
        assert!(
            crate::exit::is_invalid_input(&err),
            "explicit unknown lang must be invalid_input, got: {err}"
        );
        let msg = err.to_string();
        assert!(msg.contains("python3"), "must name the token: {msg}");
        assert!(
            !msg.to_lowercase().contains("detected from"),
            "must not blame the file path: {msg}"
        );
        assert!(msg.contains("python"), "must suggest python: {msg}");
    }

    #[test]
    fn list_sole_file_timeout_is_parse_timeout() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("deep.rs"), nested_rust_source(80_000)).unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let result = run_list(
            ListArgs {
                path: "deep.rs".into(),
                kind: None,
                compact: false,
                lang: None,
            },
            &global,
        );
        match result {
            Ok(code) => {
                panic!("sole-file list timeout must return Err(ParseTimeoutError), got Ok({code})")
            }
            Err(e) => assert!(
                crate::exit::is_parse_timeout(&e),
                "expected parse_timeout, not no_matches: {e}"
            ),
        }
    }

    #[test]
    fn list_dir_timeout_is_parse_timeout() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("deep.rs"), nested_rust_source(80_000)).unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let result = run_list(
            ListArgs {
                path: ".".into(),
                kind: None,
                compact: false,
                lang: None,
            },
            &global,
        );
        match result {
            Ok(code) => {
                panic!("dir list timeout must return Err(ParseTimeoutError), got Ok({code})")
            }
            Err(e) => assert!(
                crate::exit::is_parse_timeout(&e),
                "expected parse_timeout, not no_matches: {e}"
            ),
        }
    }

    #[test]
    fn map_dir_timeout_is_parse_timeout() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("deep.rs"), nested_rust_source(80_000)).unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let result = run_map(
            MapArgs {
                path: ".".into(),
                max_tokens: 1024,
                focus: Vec::new(),
                boost: Vec::new(),
            },
            &global,
        );
        match result {
            Ok(code) => {
                panic!("dir map timeout must return Err(ParseTimeoutError), got Ok({code})")
            }
            Err(e) => assert!(
                crate::exit::is_parse_timeout(&e),
                "expected parse_timeout, not no_matches: {e}"
            ),
        }
    }

    #[test]
    fn read_sole_file_timeout_is_parse_timeout() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("deep.rs"), nested_rust_source(80_000)).unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let result = run_read(
            ReadArgs {
                path: "deep.rs".into(),
                symbol: "main".into(),
                context: 0,
                lang: None,
            },
            &global,
        );
        match result {
            Ok(code) => {
                panic!("sole-file read timeout must return Err(ParseTimeoutError), got Ok({code})")
            }
            Err(e) => assert!(
                crate::exit::is_parse_timeout(&e),
                "expected parse_timeout, not no_matches: {e}"
            ),
        }
    }

    #[test]
    fn refs_sole_file_timeout_is_parse_timeout() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("deep.rs"), nested_rust_source(80_000)).unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let result = run_refs(
            RefsArgs {
                symbol: "main".into(),
                path: "deep.rs".into(),
                include_def: true,
                lang: None,
            },
            &global,
        );
        match result {
            Ok(code) => {
                panic!("sole-file refs timeout must return Err(ParseTimeoutError), got Ok({code})")
            }
            Err(e) => assert!(
                crate::exit::is_parse_timeout(&e),
                "expected parse_timeout, not no_matches: {e}"
            ),
        }
    }

    #[test]
    fn refs_dir_timeout_is_parse_timeout() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("deep.rs"), nested_rust_source(80_000)).unwrap();
        fs::write(dir.path().join("main.rs"), "fn helper() { main(); }\n").unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let result = run_refs(
            RefsArgs {
                symbol: "main".into(),
                path: ".".into(),
                include_def: true,
                lang: None,
            },
            &global,
        );
        match result {
            Ok(code) => {
                panic!("dir refs timeout must return Err(ParseTimeoutError), got Ok({code})")
            }
            Err(e) => assert!(
                crate::exit::is_parse_timeout(&e),
                "expected parse_timeout, not no_matches: {e}"
            ),
        }
    }

    #[test]
    fn deps_sole_file_timeout_is_parse_timeout() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("deep.rs"), nested_rust_source(80_000)).unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let result = run_deps(
            DepsArgs {
                path: "deep.rs".into(),
                reverse: false,
                lang: None,
            },
            &global,
        );
        match result {
            Ok(code) => {
                panic!("sole-file deps timeout must return Err(ParseTimeoutError), got Ok({code})")
            }
            Err(e) => assert!(
                crate::exit::is_parse_timeout(&e),
                "expected parse_timeout, not no_matches: {e}"
            ),
        }
    }

    fn git_ok(dir: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn init_git_repo_with_committed_file(dir: &std::path::Path, file: &str, content: &str) {
        git_ok(dir, &["init"]);
        git_ok(dir, &["config", "user.email", "test@test.com"]);
        git_ok(dir, &["config", "user.name", "Test"]);
        git_ok(dir, &["config", "commit.gpgsign", "false"]);
        fs::write(dir.join(file), content).unwrap();
        git_ok(dir, &["add", "--", file]);
        git_ok(dir, &["commit", "-m", "init"]);
    }

    #[test]
    fn diff_sole_file_timeout_is_parse_timeout() {
        let dir = TempDir::new().unwrap();
        init_git_repo_with_committed_file(dir.path(), "deep.rs", "fn main() {}\n");
        fs::write(dir.path().join("deep.rs"), nested_rust_source(80_000)).unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let result = run_diff(
            DiffArgs {
                path: "deep.rs".into(),
                from: "HEAD".into(),
                to: None,
                lang: None,
            },
            &global,
        );
        match result {
            Ok(code) => {
                panic!("sole-file diff timeout must return Err(ParseTimeoutError), got Ok({code})")
            }
            Err(e) => assert!(
                crate::exit::is_parse_timeout(&e),
                "expected parse_timeout, not no structural changes: {e}"
            ),
        }
    }

    #[test]
    fn deps_reverse_timeout_is_parse_timeout() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("deep.rs"), nested_rust_source(80_000)).unwrap();
        fs::write(
            dir.path().join("main.rs"),
            "use crate::deep;\nfn main() {}\n",
        )
        .unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let result = run_deps(
            DepsArgs {
                path: "deep.rs".into(),
                reverse: true,
                lang: None,
            },
            &global,
        );
        match result {
            Ok(code) => {
                panic!("reverse deps timeout must return Err(ParseTimeoutError), got Ok({code})")
            }
            Err(e) => assert!(
                crate::exit::is_parse_timeout(&e),
                "expected parse_timeout, not no_matches/empty: {e}"
            ),
        }
    }

    #[test]
    fn deps_reverse_unreadable_sibling_is_invalid_input() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("target.rs"), "fn foo() {}\n").unwrap();
        let locked = dir.path().join("locked.rs");
        fs::write(&locked, "fn bar() {}\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
            // Root (common in Docker) can still read mode-000 files. Skip when
            // permissions do not actually block reading.
            if fs::read_to_string(&locked).is_ok() {
                fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
                return;
            }
            let global = GlobalFlags {
                json: true,
                quiet: true,
                ..GlobalFlags::test_with_cwd(dir.path())
            };
            let result = run_deps(
                DepsArgs {
                    path: "target.rs".into(),
                    reverse: true,
                    lang: None,
                },
                &global,
            );
            match result {
                Ok(code) => {
                    assert_eq!(
                        code,
                        exit::FAILURE,
                        "reverse scan must surface invalid_input (exit {}), not no_matches ({})",
                        exit::FAILURE,
                        exit::NO_MATCHES
                    );
                    assert_ne!(
                        code,
                        exit::NO_MATCHES,
                        "must not report no_matches when a scanned sibling is unreadable"
                    );
                }
                Err(e) => {
                    panic!("unreadable sibling must be Ok(FAILURE) invalid_input, not Err({e})")
                }
            }
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
        }
        #[cfg(not(unix))]
        {
            let _ = locked;
        }
    }

    #[test]
    fn impact_sole_file_timeout_is_parse_timeout() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("deep.rs"), nested_rust_source(80_000)).unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let result = run_impact(
            ImpactArgs {
                symbol: "main".into(),
                path: "deep.rs".into(),
                depth: 3,
                lang: None,
            },
            &global,
        );
        match result {
            Ok(code) => {
                panic!(
                    "sole-file impact timeout must return Err(ParseTimeoutError), got Ok({code})"
                )
            }
            Err(e) => assert!(
                crate::exit::is_parse_timeout(&e),
                "expected parse_timeout, not no_matches: {e}"
            ),
        }
    }
}
