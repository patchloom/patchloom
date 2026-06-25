//! Symbol-scoped replacement: replace text only within a specific symbol's body.

use std::path::Path;

use super::Language;
use super::symbols::{extract_symbols, find_symbol};

/// Result of a symbol-scoped replacement.
#[derive(Debug)]
pub struct ScopedReplaceResult {
    /// The full file content after replacement.
    pub content: String,
    /// Number of replacements made within the symbol.
    pub replacements: usize,
    /// 1-based start line of the symbol.
    pub start_line: usize,
    /// 1-based end line of the symbol.
    pub end_line: usize,
}

/// Replace text only within the body of a specific symbol.
pub fn replace_in_symbol(
    source: &str,
    symbol_name: &str,
    from: &str,
    to: &str,
    regex: bool,
    lang: Language,
) -> anyhow::Result<Option<ScopedReplaceResult>> {
    let symbols = extract_symbols(source, lang);
    let sym = match find_symbol(&symbols, symbol_name) {
        Some(s) => s,
        None => return Ok(None),
    };

    let lines: Vec<&str> = source.lines().collect();
    let start_idx = sym.start_line.saturating_sub(1);
    let end_idx = sym.end_line.min(lines.len());

    let body: String = lines[start_idx..end_idx]
        .iter()
        .map(|l| format!("{l}\n"))
        .collect();

    // Perform replacement within the body
    let (new_body, count) = if regex {
        let re = regex::Regex::new(from)?;
        let count = re.find_iter(&body).count();
        let new = re.replace_all(&body, to).into_owned();
        (new, count)
    } else {
        let count = body.matches(from).count();
        let new = body.replace(from, to);
        (new, count)
    };

    if count == 0 {
        return Ok(Some(ScopedReplaceResult {
            content: source.to_string(),
            replacements: 0,
            start_line: sym.start_line,
            end_line: sym.end_line,
        }));
    }

    // Reconstruct the full file: prefix + new body + suffix
    let mut result = String::new();
    for line in &lines[..start_idx] {
        result.push_str(line);
        result.push('\n');
    }
    result.push_str(&new_body);
    // new_body already has trailing newlines; add suffix
    for line in &lines[end_idx..] {
        result.push_str(line);
        result.push('\n');
    }

    // Preserve original trailing newline behavior
    if !source.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    Ok(Some(ScopedReplaceResult {
        content: result,
        replacements: count,
        start_line: sym.start_line,
        end_line: sym.end_line,
    }))
}

/// Replace text within a symbol in a file.
pub fn replace_in_symbol_file(
    path: &Path,
    symbol_name: &str,
    from: &str,
    to: &str,
    regex: bool,
    lang_hint: Option<Language>,
) -> anyhow::Result<Option<ScopedReplaceResult>> {
    let lang = lang_hint.unwrap_or_else(|| Language::from_path(path));
    if !lang.has_grammar() {
        return Ok(None);
    }
    let source = std::fs::read_to_string(path)?;
    replace_in_symbol(&source, symbol_name, from, to, regex, lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_within_function() {
        let source = r#"fn foo() {
    let x = 1;
    let y = 1;
}

fn bar() {
    let x = 1;
}
"#;
        let result = replace_in_symbol(source, "foo", "1", "42", false, Language::Rust)
            .unwrap()
            .unwrap();
        assert_eq!(result.replacements, 2);
        // foo's body should have 42, bar's should still have 1
        assert!(result.content.contains("fn foo()"));
        assert!(result.content.contains("let x = 42"));
        let bar_section: &str = result.content.split("fn bar()").nth(1).unwrap();
        assert!(bar_section.contains("let x = 1"));
    }

    #[test]
    fn replace_with_regex() {
        let source = "fn run() {\n    exit::FAILURE\n    exit::NO_MATCHES\n}\n";
        let result = replace_in_symbol(
            source,
            "run",
            r"exit::\w+",
            "exit::SUCCESS",
            true,
            Language::Rust,
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.replacements, 2);
        assert!(result.content.contains("exit::SUCCESS"));
        assert!(!result.content.contains("exit::FAILURE"));
    }

    #[test]
    fn symbol_not_found_returns_none() {
        let source = "fn foo() {}\n";
        let result =
            replace_in_symbol(source, "nonexistent", "x", "y", false, Language::Rust).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn no_match_returns_zero_replacements() {
        let source = "fn foo() {\n    let x = 1;\n}\n";
        let result = replace_in_symbol(source, "foo", "zzz", "yyy", false, Language::Rust)
            .unwrap()
            .unwrap();
        assert_eq!(result.replacements, 0);
    }
}
