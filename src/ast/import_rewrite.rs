//! Rewrite consumer import/use statements after `ast.move` / `ast.extract_to_file`.
//!
//! Detection uses [`super::imports::list_imports`]. This module only rewrites
//! already-found import blocks (Rust `use` first).

use super::Language;
use super::imports::list_imports;

/// One symbol that moved from `old_module` to `new_module`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolMove {
    /// Identifier as it appears in `use path::Name` (not the `as` alias).
    pub name: String,
    /// Module path consumers currently import from (e.g. `crate::old_mod`).
    pub old_module: String,
    /// Module path consumers should import from (e.g. `crate::new_mod`).
    pub new_module: String,
}

/// Rewrite import blocks in `source` for the given symbol moves.
///
/// Returns `None` when no import text changes (no matching consumer, glob
/// left as-is, or language not rewritten).
pub fn rewrite_imports_in_source(
    source: &str,
    lang: Language,
    moves: &[SymbolMove],
) -> Option<String> {
    if moves.is_empty() || !matches!(lang, Language::Rust) {
        return None;
    }
    let imports = list_imports(source, lang);
    if imports.is_empty() {
        return None;
    }

    let lines: Vec<&str> = source.lines().collect();
    let mut replacements: Vec<(usize, usize, String)> = Vec::new();
    for import in &imports {
        let Some(rewritten) = rewrite_rust_use_block(&import.text, moves) else {
            continue;
        };
        let start = import.line.saturating_sub(1);
        if start >= lines.len() {
            continue;
        }
        let count = import.text.lines().count().max(1);
        let end = (start + count).min(lines.len());
        replacements.push((start, end, rewritten));
    }
    if replacements.is_empty() {
        return None;
    }
    Some(apply_line_replacements(source, &lines, &replacements))
}

fn apply_line_replacements(
    source: &str,
    lines: &[&str],
    replacements: &[(usize, usize, String)],
) -> String {
    let eol = crate::write::detect_eol(source);
    let mut out = String::new();
    let mut i = 0usize;
    let mut r = 0usize;
    while i < lines.len() {
        if r < replacements.len() && i == replacements[r].0 {
            let (_, end, ref text) = replacements[r];
            out.push_str(text);
            if !text.is_empty() && !text.ends_with('\n') {
                // Keep a trailing newline after the replacement unless this
                // block was the last line of a file that had no final newline.
                if end < lines.len() || source.ends_with('\n') {
                    out.push_str(eol);
                }
            }
            i = end;
            r += 1;
        } else {
            out.push_str(lines[i]);
            if i + 1 < lines.len() || source.ends_with('\n') {
                out.push_str(eol);
            }
            i += 1;
        }
    }
    out
}

/// Rewrite one Rust `use` block (single- or multi-line text from `list_imports`).
fn rewrite_rust_use_block(text: &str, moves: &[SymbolMove]) -> Option<String> {
    let compact = flatten_import_text(text);
    let (vis, body) = rust_use_prefix_and_body(&compact)?;
    if body.ends_with("::*") || body.contains("::*") && !body.contains('{') {
        return None;
    }
    if let Some(brace) = body.find('{') {
        let close = body.rfind('}')?;
        if close < brace {
            return None;
        }
        let path = body[..brace].trim().trim_end_matches(':').trim();
        let inner = &body[brace + 1..close];
        if inner.split(',').any(is_glob_item) {
            return None;
        }
        return rewrite_grouped_use(vis, path, inner, moves);
    }
    rewrite_simple_use(vis, body, moves)
}

fn flatten_import_text(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn rust_use_prefix_and_body(text: &str) -> Option<(&str, &str)> {
    let trimmed = text.trim();
    let vis = if trimmed.starts_with("pub(crate) use ") {
        "pub(crate) use "
    } else if trimmed.starts_with("pub(super) use ") {
        "pub(super) use "
    } else if trimmed.starts_with("pub use ") {
        "pub use "
    } else if trimmed.starts_with("use ") {
        "use "
    } else {
        return None;
    };
    let rest = trimmed.get(vis.len()..)?.trim();
    let rest = rest.strip_suffix(';').unwrap_or(rest).trim();
    Some((vis, rest))
}

fn rewrite_simple_use(vis: &str, body: &str, moves: &[SymbolMove]) -> Option<String> {
    for mv in moves {
        let old = normalize_module(&mv.old_module);
        let Some(tail) = module_tail(body, &old) else {
            continue;
        };
        if tail.is_empty() || tail == "*" || tail.contains("::") {
            continue;
        }
        let (name, _) = split_name_alias(tail);
        if name != mv.name {
            continue;
        }
        let new = normalize_module(&mv.new_module);
        return Some(format!("{vis}{new}::{tail};"));
    }
    None
}

fn rewrite_grouped_use(vis: &str, path: &str, inner: &str, moves: &[SymbolMove]) -> Option<String> {
    let path = normalize_module(path);
    let items: Vec<&str> = inner
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if items.is_empty() {
        return None;
    }

    let mut remaining: Vec<&str> = Vec::new();
    let mut moved: Vec<(&str, &SymbolMove)> = Vec::new();
    for item in &items {
        let (name, _) = split_name_alias(item);
        match moves
            .iter()
            .find(|mv| normalize_module(&mv.old_module) == path && mv.name == name)
        {
            Some(mv) => moved.push((item, mv)),
            None => remaining.push(item),
        }
    }
    if moved.is_empty() {
        return None;
    }

    let mut parts: Vec<String> = Vec::new();
    if !remaining.is_empty() {
        parts.push(format_use_items(vis, &path, &remaining));
    }
    // Group moved items by destination module, preserving encounter order.
    let mut dest_items: Vec<(String, Vec<&str>)> = Vec::new();
    for (item, mv) in moved {
        let dest = normalize_module(&mv.new_module);
        if let Some((_, bucket)) = dest_items.iter_mut().find(|(d, _)| *d == dest) {
            bucket.push(item);
        } else {
            dest_items.push((dest, vec![item]));
        }
    }
    for (dest, names) in dest_items {
        parts.push(format_use_items(vis, &dest, &names));
    }
    Some(parts.join("\n"))
}

fn format_use_items(vis: &str, module: &str, items: &[&str]) -> String {
    if items.len() == 1 {
        format!("{vis}{module}::{};", items[0])
    } else {
        format!("{vis}{module}::{{{}}};", items.join(", "))
    }
}

fn normalize_module(module: &str) -> String {
    module.trim().trim_end_matches(':').trim().to_string()
}

fn module_tail<'a>(path: &'a str, module: &str) -> Option<&'a str> {
    if path == module {
        return Some("");
    }
    path.strip_prefix(module)
        .and_then(|rest| rest.strip_prefix("::"))
}

fn split_name_alias(item: &str) -> (&str, Option<&str>) {
    let item = strip_line_comment(item);
    let mut parts = item.split_whitespace();
    let name = parts.next().unwrap_or("");
    let alias = match (parts.next(), parts.next()) {
        (Some(kw), Some(alias)) if kw.eq_ignore_ascii_case("as") => Some(alias),
        _ => None,
    };
    (name, alias)
}

fn strip_line_comment(s: &str) -> &str {
    match s.find("//") {
        Some(i) => s[..i].trim(),
        None => s.trim(),
    }
}

fn is_glob_item(part: &str) -> bool {
    let name = split_name_alias(part).0;
    name == "*"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rust_move(name: &str) -> SymbolMove {
        SymbolMove {
            name: name.into(),
            old_module: "crate::old_mod".into(),
            new_module: "crate::new_mod".into(),
        }
    }

    #[test]
    fn rewrite_simple_use_line() {
        let source = "use crate::old_mod::helper;\n\nfn main() {}\n";
        let out = rewrite_imports_in_source(source, Language::Rust, &[rust_move("helper")])
            .expect("should rewrite simple use");
        assert_eq!(out, "use crate::new_mod::helper;\n\nfn main() {}\n");
    }

    #[test]
    fn rewrite_grouped_partial_move() {
        let source = "use crate::old_mod::{alpha, beta, gamma};\n\nfn main() {}\n";
        let moves = [rust_move("alpha"), rust_move("gamma")];
        let out = rewrite_imports_in_source(source, Language::Rust, &moves)
            .expect("should rewrite grouped use");
        assert_eq!(
            out,
            "use crate::old_mod::beta;\nuse crate::new_mod::{alpha, gamma};\n\nfn main() {}\n"
        );
    }

    #[test]
    fn rewrite_preserves_visibility_prefix() {
        let source = "pub use crate::old_mod::helper;\npub(crate) use crate::old_mod::helper;\n";
        let out = rewrite_imports_in_source(source, Language::Rust, &[rust_move("helper")])
            .expect("should rewrite vis use");
        assert_eq!(
            out,
            "pub use crate::new_mod::helper;\npub(crate) use crate::new_mod::helper;\n"
        );
    }

    #[test]
    fn rewrite_leaves_indented_use_unchanged() {
        let source = "fn main() {\n    use crate::old_mod::helper;\n}\n";
        let out = rewrite_imports_in_source(source, Language::Rust, &[rust_move("helper")]);
        assert_eq!(out, None);
    }

    #[test]
    fn rewrite_no_matching_consumer_is_unchanged() {
        let source = "use crate::other::foo;\n\nfn main() {}\n";
        let out = rewrite_imports_in_source(source, Language::Rust, &[rust_move("helper")]);
        assert_eq!(out, None);
    }

    #[test]
    fn rewrite_keeps_name_as_alias() {
        let source = "use crate::old_mod::Name as Alias;\n\nfn main() {}\n";
        let out = rewrite_imports_in_source(source, Language::Rust, &[rust_move("Name")])
            .expect("should rewrite aliased use");
        assert_eq!(out, "use crate::new_mod::Name as Alias;\n\nfn main() {}\n");
    }

    #[test]
    fn rewrite_leaves_glob_use_unchanged() {
        let source = "use crate::old_mod::*;\n\nfn main() {}\n";
        let out = rewrite_imports_in_source(source, Language::Rust, &[rust_move("helper")]);
        assert_eq!(out, None);
    }
}
