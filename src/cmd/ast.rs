//! AST-aware subcommands: `patchloom ast list|read|rename|validate|search|refs|deps`.

use crate::ast::Language;
use crate::ast::symbols::{self, SymbolDef, SymbolKind};
use crate::cli::global::GlobalFlags;
use crate::exit;
use clap::{Args, Subcommand};

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
    /// Structural search using tree-sitter queries.
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
    let lang_hint = args.lang.as_deref().map(lang_from_str);

    let mut any_output = false;

    if target.is_file() {
        let symbols = symbols::extract_symbols_from_file(&target, lang_hint);
        let filtered = filter_symbols(&symbols, &kind_filter);
        if !filtered.is_empty() {
            any_output = true;
            if global.json || global.jsonl {
                print_symbols_json(&args.path, &filtered, global.jsonl);
            } else if args.compact {
                print_symbols_compact(&args.path, &filtered);
            } else {
                print_symbols_human(&args.path, &filtered);
            }
        }
    } else if target.is_dir() {
        let paths = collect_source_files(&target, global)?;
        for path in &paths {
            let lang = lang_hint.unwrap_or_else(|| Language::from_path(path));
            let symbols = symbols::extract_symbols_from_file(path, Some(lang));
            let filtered = filter_symbols(&symbols, &kind_filter);
            if filtered.is_empty() {
                continue;
            }
            any_output = true;
            let display_path = path.strip_prefix(&cwd).unwrap_or(path);
            let display = display_path.display().to_string();
            if global.json || global.jsonl {
                print_symbols_json(&display, &filtered, global.jsonl);
            } else if args.compact {
                print_symbols_compact(&display, &filtered);
            } else {
                print_symbols_human(&display, &filtered);
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
    let cwd = global.resolve_cwd()?;
    let target = cwd.join(&args.path);

    let lang_hint = args.lang.as_deref().map(lang_from_str);
    let lang = lang_hint.unwrap_or_else(|| Language::from_path(&target));
    let source = std::fs::read_to_string(&target)?;
    let all_symbols = symbols::extract_symbols(&source, lang);
    let sym = symbols::find_symbol(&all_symbols, &args.symbol)
        .ok_or_else(|| anyhow::anyhow!("symbol '{}' not found in {}", args.symbol, args.path))?;

    let lines: Vec<&str> = source.lines().collect();
    let start = sym.start_line.saturating_sub(1 + args.context);
    let end = (sym.end_line + args.context).min(lines.len());

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
        if global.jsonl {
            println!("{}", serde_json::to_string(&obj)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&obj)?);
        }
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

fn run_rename(args: RenameArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    let target = cwd.join(&args.path);
    let lang_hint = args.lang.as_deref().map(lang_from_str);

    let mut total_replacements = 0usize;
    let mut files_changed = 0usize;

    let paths = if target.is_file() {
        vec![target.clone()]
    } else if target.is_dir() {
        collect_source_files(&target, global)?
    } else {
        anyhow::bail!("path not found: {}", args.path);
    };

    for path in &paths {
        let lang = lang_hint.unwrap_or_else(|| Language::from_path(path));
        let source = std::fs::read_to_string(path)?;
        let result = if lang.has_grammar() {
            crate::ast::rename::rename_in_source(&source, &args.old_name, &args.new_name, lang)
        } else {
            // Fallback to word-boundary replace
            None
        };

        let (new_content, count) = match result {
            Some(r) if r.replacements > 0 => (r.content, r.replacements),
            _ => {
                // Try word-boundary fallback for unknown languages or zero AST matches
                let re = crate::ops::replace::compile_replace_regex(
                    &args.old_name,
                    false,
                    false,
                    false,
                    true,
                )?;
                if let Some(re) = re {
                    let new = re.replace_all(&source, args.new_name.as_str());
                    let count = re.find_iter(&source).count();
                    if count > 0 {
                        (new.into_owned(), count)
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            }
        };

        total_replacements += count;
        files_changed += 1;
        let display_path = path.strip_prefix(&cwd).unwrap_or(path);

        if global.apply {
            crate::write::atomic_write(path, &new_content, &Default::default())?;
            if !global.quiet {
                eprintln!(
                    "{}: {} replacement{}",
                    display_path.display(),
                    count,
                    if count == 1 { "" } else { "s" }
                );
            }
        } else if global.check {
            if !global.quiet {
                eprintln!(
                    "{}: {} replacement{} (check mode)",
                    display_path.display(),
                    count,
                    if count == 1 { "" } else { "s" }
                );
            }
        } else {
            // Default dry-run: show diff
            let diff = crate::diff::unified_diff(
                &display_path.display().to_string(),
                &source,
                &new_content,
            );
            if diff.has_changes {
                print!("{}", diff.hunks);
            }
        }
    }

    if global.json || global.jsonl {
        let obj = serde_json::json!({
            "ok": true,
            "files_changed": files_changed,
            "replacements": total_replacements,
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    }

    if total_replacements == 0 {
        Ok(exit::NO_MATCHES)
    } else if global.check {
        Ok(exit::CHANGES_DETECTED)
    } else {
        Ok(exit::SUCCESS)
    }
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
    let cwd = global.resolve_cwd()?;
    let target = cwd.join(&args.path);
    let lang_hint = args.lang.as_deref().map(lang_from_str);

    let mut all_valid = true;

    let paths = if target.is_file() {
        vec![target.clone()]
    } else if target.is_dir() {
        collect_source_files(&target, global)?
    } else {
        anyhow::bail!("path not found: {}", args.path);
    };

    for path in &paths {
        let lang = lang_hint.unwrap_or_else(|| Language::from_path(path));
        if !lang.has_grammar() {
            continue;
        }
        let result = crate::ast::validate::validate_file(path, Some(lang))?;
        let display_path = path.strip_prefix(&cwd).unwrap_or(path);

        if global.json || global.jsonl {
            let obj = serde_json::json!({
                "file": display_path.display().to_string(),
                "valid": result.valid,
                "language": result.language,
                "errors": result.errors,
            });
            if global.jsonl {
                println!("{}", serde_json::to_string(&obj)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&obj)?);
            }
        } else if !result.valid {
            all_valid = false;
            eprintln!("{}: INVALID ({})", display_path.display(), result.language);
            for err in &result.errors {
                eprintln!("  line {}:{}: {}", err.line, err.column, err.text.trim());
            }
        } else if !global.quiet {
            eprintln!("{}: OK ({})", display_path.display(), result.language);
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
    let cwd = global.resolve_cwd()?;
    let target = cwd.join(&args.path);
    let lang_hint = args.lang.as_deref().map(lang_from_str);

    let mut total_matches = 0usize;

    let paths = if target.is_file() {
        vec![target.clone()]
    } else if target.is_dir() {
        collect_source_files(&target, global)?
    } else {
        anyhow::bail!("path not found: {}", args.path);
    };

    for path in &paths {
        let lang = lang_hint.unwrap_or_else(|| Language::from_path(path));

        let query_str = if args.pattern {
            crate::ast::search::compile_pattern_query(&args.query, lang)?
        } else {
            args.query.clone()
        };

        let results =
            crate::ast::search::search_file(path, &query_str, Some(lang), args.max_results)?;
        if results.is_empty() {
            continue;
        }

        let display_path = path.strip_prefix(&cwd).unwrap_or(path);
        let display = display_path.display().to_string();

        for m in &results {
            total_matches += 1;
            if global.json || global.jsonl {
                let obj = serde_json::json!({
                    "file": display,
                    "line": m.line,
                    "column": m.column,
                    "text": m.text,
                    "captures": m.captures,
                });
                if global.jsonl {
                    println!("{}", serde_json::to_string(&obj)?);
                } else {
                    println!("{}", serde_json::to_string_pretty(&obj)?);
                }
            } else {
                println!(
                    "{}:{}:{}: {}",
                    display,
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
    let cwd = global.resolve_cwd()?;
    let target = cwd.join(&args.path);
    let lang_hint = args.lang.as_deref().map(lang_from_str);

    let paths = if target.is_file() {
        vec![target.clone()]
    } else if target.is_dir() {
        collect_source_files(&target, global)?
    } else {
        anyhow::bail!("path not found: {}", args.path);
    };

    let mut all_refs = Vec::new();

    for path in &paths {
        let display_path = path.strip_prefix(&cwd).unwrap_or(path);
        let display = display_path.display().to_string();
        let refs = crate::ast::refs::find_refs_in_file(path, &args.symbol, lang_hint, &display);
        all_refs.extend(refs);
    }

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
        if global.jsonl {
            println!("{}", serde_json::to_string(&obj)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&obj)?);
        }
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

    let paths = if target.is_file() {
        vec![target.clone()]
    } else if target.is_dir() {
        collect_source_files(&target, global)?
    } else {
        anyhow::bail!("path not found: {}", args.path);
    };

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
            let display_path = path.strip_prefix(&cwd).unwrap_or(path);
            let display = display_path.display().to_string();

            if global.json || global.jsonl {
                for imp in &matching {
                    let obj = serde_json::json!({
                        "file": display,
                        "imports": imp.path,
                        "line": imp.line,
                        "raw": imp.raw,
                    });
                    if global.jsonl {
                        println!("{}", serde_json::to_string(&obj)?);
                    } else {
                        println!("{}", serde_json::to_string_pretty(&obj)?);
                    }
                }
            } else {
                for imp in &matching {
                    println!("{}:{}: {}", display, imp.line, imp.raw);
                }
            }
        }
    } else {
        for path in &paths {
            let imports = crate::ast::deps::extract_imports_from_file(path, lang_hint);
            if imports.is_empty() {
                continue;
            }
            any_output = true;
            let display_path = path.strip_prefix(&cwd).unwrap_or(path);
            let display = display_path.display().to_string();

            if global.json || global.jsonl {
                let obj = serde_json::json!({
                    "file": display,
                    "imports": imports,
                });
                if global.jsonl {
                    println!("{}", serde_json::to_string(&obj)?);
                } else {
                    println!("{}", serde_json::to_string_pretty(&obj)?);
                }
            } else {
                println!("{display}");
                println!("  imports:");
                for imp in &imports {
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

    if !target.is_dir() {
        anyhow::bail!("path must be a directory: {}", args.path);
    }

    let paths = collect_source_files(&target, global)?;
    let file_pairs: Vec<(std::path::PathBuf, String)> = paths
        .iter()
        .map(|p| {
            let display = p.strip_prefix(&cwd).unwrap_or(p).display().to_string();
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

    if global.json || global.jsonl {
        if global.jsonl {
            for entry in &entries {
                println!("{}", serde_json::to_string(entry)?);
            }
        } else {
            println!("{}", serde_json::to_string_pretty(&entries)?);
        }
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

fn run_replace(args: ReplaceArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    let target = cwd.join(&args.path);
    let lang_hint = args.lang.as_deref().map(lang_from_str);

    let source = std::fs::read_to_string(&target)?;
    let lang = lang_hint.unwrap_or_else(|| Language::from_path(&target));

    let result = crate::ast::replace::replace_in_symbol(
        &source,
        &args.symbol,
        &args.from,
        &args.to,
        args.regex,
        lang,
    )?;

    let result = match result {
        Some(r) => r,
        None => {
            if !global.quiet {
                eprintln!("symbol '{}' not found in {}", args.symbol, args.path);
            }
            return Ok(exit::NO_MATCHES);
        }
    };

    if result.replacements == 0 {
        return Ok(exit::NO_MATCHES);
    }

    let display_path = target.strip_prefix(&cwd).unwrap_or(&target);

    if global.apply {
        crate::write::atomic_write(&target, &result.content, &Default::default())?;
        if !global.quiet {
            eprintln!(
                "{}: {} replacement{} in symbol '{}'",
                display_path.display(),
                result.replacements,
                if result.replacements == 1 { "" } else { "s" },
                args.symbol,
            );
        }
    } else if global.check {
        if !global.quiet {
            eprintln!(
                "{}: {} replacement{} in symbol '{}' (check mode)",
                display_path.display(),
                result.replacements,
                if result.replacements == 1 { "" } else { "s" },
                args.symbol,
            );
        }
    } else {
        let diff = crate::diff::unified_diff(
            &display_path.display().to_string(),
            &source,
            &result.content,
        );
        if diff.has_changes {
            print!("{}", diff.hunks);
        }
    }

    if global.json || global.jsonl {
        let obj = serde_json::json!({
            "ok": true,
            "symbol": args.symbol,
            "replacements": result.replacements,
            "start_line": result.start_line,
            "end_line": result.end_line,
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    }

    if global.check {
        Ok(exit::CHANGES_DETECTED)
    } else {
        Ok(exit::SUCCESS)
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
    let cwd = global.resolve_cwd()?;
    let target = cwd.join(&args.path);

    let paths = if target.is_file() {
        vec![target.clone()]
    } else if target.is_dir() {
        collect_source_files(&target, global)?
    } else {
        anyhow::bail!("path not found: {}", args.path);
    };

    let file_pairs: Vec<(std::path::PathBuf, String)> = paths
        .iter()
        .map(|p| {
            let display = p.strip_prefix(&cwd).unwrap_or(p).display().to_string();
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
        if global.jsonl {
            println!("{}", serde_json::to_string(&obj)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&obj)?);
        }
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
    let lang_hint = args.lang.as_deref().map(lang_from_str);
    let lang = lang_hint.unwrap_or_else(|| Language::from_path(&target));

    // Get old version from git
    let old_source = get_git_file_content(&cwd, &args.path, &args.from)?;

    // Get new version
    let new_source = if let Some(ref to_ref) = args.to {
        get_git_file_content(&cwd, &args.path, to_ref)?
    } else {
        std::fs::read_to_string(&target)?
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
        if global.jsonl {
            println!("{}", serde_json::to_string(&obj)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&obj)?);
        }
    } else {
        print!("{}", crate::ast::diff::render_changes(&args.path, &changes));
    }

    Ok(exit::SUCCESS)
}

fn get_git_file_content(
    cwd: &std::path::Path,
    file_path: &str,
    git_ref: &str,
) -> anyhow::Result<String> {
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

fn lang_from_str(s: &str) -> Language {
    Language::from_extension(s)
}

fn parse_kind_filter(kind_arg: &Option<String>) -> Vec<SymbolKind> {
    match kind_arg {
        Some(s) => s
            .split(',')
            .filter_map(|k| SymbolKind::from_str_loose(k.trim()))
            .collect(),
        None => Vec::new(),
    }
}

fn filter_symbols<'a>(symbols: &'a [SymbolDef], kind_filter: &[SymbolKind]) -> Vec<&'a SymbolDef> {
    if kind_filter.is_empty() {
        return symbols.iter().collect();
    }
    symbols
        .iter()
        .filter(|s| kind_filter.contains(&s.kind))
        .collect()
}

fn collect_source_files(
    dir: &std::path::Path,
    global: &GlobalFlags,
) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let mut paths = Vec::new();
    let glob_matcher = crate::files::build_glob_matcher(global)?;

    let walker = ignore::WalkBuilder::new(dir)
        .hidden(true)
        .git_ignore(true)
        .build();

    for entry in walker {
        let entry = entry?;
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        let lang = Language::from_path(path);
        if !lang.has_grammar() {
            continue;
        }
        if !crate::files::matches_glob(path, glob_matcher.as_ref()) {
            continue;
        }
        paths.push(path.to_path_buf());
    }
    paths.sort();
    Ok(paths)
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

fn print_symbols_json(path: &str, symbols: &[&SymbolDef], jsonl: bool) {
    for sym in symbols {
        let obj = symbol_to_json(sym, path);
        if jsonl {
            println!("{}", serde_json::to_string(&obj).unwrap_or_default());
        } else {
            println!("{}", serde_json::to_string_pretty(&obj).unwrap_or_default());
        }
    }
}

fn symbol_to_json(sym: &SymbolDef, path: &str) -> serde_json::Value {
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
}
