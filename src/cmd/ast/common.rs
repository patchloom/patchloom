//! Shared helpers for `patchloom ast` subcommands.

use crate::ast::Language;
use crate::ast::symbols::SymbolDef;
use crate::cli::global::GlobalFlags;
use std::path::{Path, PathBuf};

/// Resolve `--lang` hint to a `Language`, falling back to extension detection.
pub(super) fn resolve_lang(lang_arg: Option<&str>, path: &Path) -> Language {
    lang_arg
        .map(lang_from_str)
        .unwrap_or_else(|| Language::from_path(path))
}

/// Common preamble for single-file AST commands: resolve cwd, join path, detect
/// language, read source. Returns `(cwd, target, lang, source)`.
pub(super) fn setup_single_file(
    path_arg: &str,
    lang_arg: Option<&str>,
    global: &GlobalFlags,
) -> anyhow::Result<(PathBuf, PathBuf, Language, String)> {
    let cwd = global.resolve_cwd()?;
    global.check_paths_contained(&cwd, [path_arg])?;
    let target = cwd.join(path_arg);
    // Strict sole-path text load (#1894): binary / invalid UTF-8 → InvalidInput.
    let source = crate::files::load_text_strict(&target, path_arg)?;
    let lang = resolve_lang(lang_arg, &target);
    Ok((cwd, target, lang, source))
}

/// When the user names exactly one file path that is binary, fail as
/// `invalid_input` (parity with replace/search/tidy/md). Directory walks still
/// soft-skip non-source files.
pub(super) fn reject_sole_explicit_binary(
    paths: &[PathBuf],
    path_arg: &str,
) -> Result<(), crate::exit::InvalidInputError> {
    if paths.len() != 1 {
        return Ok(());
    }
    crate::ops::file::ensure_not_binary_file(&paths[0], path_arg)
}

/// Common preamble for multi-file AST commands: resolve cwd, join path, resolve
/// target paths. Returns `(cwd, paths)`.
pub(super) fn setup_multi_file(
    path_arg: &str,
    global: &GlobalFlags,
) -> anyhow::Result<(PathBuf, Vec<PathBuf>)> {
    let cwd = global.resolve_cwd()?;
    global.check_paths_contained(&cwd, [path_arg])?;
    let target = cwd.join(path_arg);
    let paths = resolve_target_paths(&target, path_arg, global)?;
    Ok((cwd, paths))
}

pub(crate) fn display_path(path: &Path, cwd: &Path) -> String {
    crate::files::relative_display(path, cwd)
        .display()
        .to_string()
}

pub(crate) fn resolve_target_paths(
    target: &std::path::Path,
    path_arg: &str,
    global: &GlobalFlags,
) -> anyhow::Result<Vec<std::path::PathBuf>> {
    if target.is_file() {
        Ok(vec![target.to_path_buf()])
    } else if target.is_dir() {
        collect_source_files(target, global)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("path not found: {path_arg}"),
        )
        .into())
    }
}

pub(crate) fn get_git_file_content(
    cwd: &std::path::Path,
    file_path: &str,
    git_ref: &str,
) -> anyhow::Result<String> {
    if git_ref.starts_with('-') {
        return Err(crate::exit::InvalidInputError {
            msg: "invalid git ref: must not start with '-'".into(),
        }
        .into());
    }
    let output = std::process::Command::new("git")
        .args(["show", &format!("{git_ref}:{file_path}")])
        .current_dir(cwd)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("git show {git_ref}:{file_path} failed: {}", stderr.trim()),
        )
        .into());
    }

    Ok(String::from_utf8(output.stdout)?)
}

pub(crate) fn lang_from_str(s: &str) -> Language {
    Language::from_name_or_ext(s)
}

pub use crate::ast::symbols::{filter_symbols, parse_kind_filter};

pub(crate) fn collect_source_files(
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

pub(super) fn print_symbols_human(path: &str, symbols: &[&SymbolDef]) {
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

pub(super) fn print_symbols_compact(path: &str, symbols: &[&SymbolDef]) {
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

/// Emit symbols in agent-honest structured form.
///
/// `--json` prints a single pretty-printed JSON array (one `json.loads` for
/// the whole stdout). `--jsonl` prints one compact object per line.
/// Per-symbol `emit_json` was wrong for multi-symbol files: pretty multi-docs
/// are not a single parseable JSON value (fixrealloop 2026-07-15).
pub(super) fn print_symbols_json(
    path: &str,
    symbols: &[&SymbolDef],
    global: &GlobalFlags,
) -> anyhow::Result<()> {
    let items: Vec<serde_json::Value> = symbols
        .iter()
        .map(|sym| symbol_to_json(sym, path))
        .collect();
    global.emit_json_items(&items)?;
    Ok(())
}

/// Emit a pre-built list of symbol JSON objects (multi-file list path).
pub(super) fn print_symbol_items_json(
    items: &[serde_json::Value],
    global: &GlobalFlags,
) -> anyhow::Result<()> {
    global.emit_json_items(items)?;
    Ok(())
}

pub(crate) fn symbol_to_json(sym: &SymbolDef, path: &str) -> serde_json::Value {
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

    /// Language names (not just extensions) should be accepted (#1165).
    #[test]
    fn lang_from_str_language_names() {
        assert_eq!(lang_from_str("rust"), Language::Rust);
        assert_eq!(lang_from_str("python"), Language::Python);
        assert_eq!(lang_from_str("typescript"), Language::TypeScript);
        assert_eq!(lang_from_str("javascript"), Language::JavaScript);
        assert_eq!(lang_from_str("golang"), Language::Go);
        assert_eq!(lang_from_str("csharp"), Language::CSharp);
        assert_eq!(lang_from_str("ruby"), Language::Ruby);
        assert_eq!(lang_from_str("kotlin"), Language::Kotlin);
        assert_eq!(lang_from_str("terraform"), Language::Hcl);
        assert_eq!(lang_from_str("markdown"), Language::Markdown);
        assert_eq!(lang_from_str("Rust"), Language::Rust); // case-insensitive
    }

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
        assert!(
            crate::exit::is_io_not_found(&err),
            "missing path should preserve NotFound: {err}"
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
}
