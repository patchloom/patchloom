//! Import/use statement manipulation: add, remove, deduplicate.

use super::Language;

/// Result of an imports operation.
#[derive(Debug)]
pub struct ImportsResult {
    /// The full file content after modification.
    pub content: String,
    /// Number of imports added.
    pub added: usize,
    /// Number of imports removed.
    pub removed: usize,
    /// Number of duplicates merged.
    pub deduped: usize,
}

/// An import statement found in source code.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImportStatement {
    /// The full import text (e.g. "use std::io;").
    pub text: String,
    /// 1-based line number.
    pub line: usize,
}

/// List all import/use statements in source code.
pub fn list_imports(source: &str, lang: Language) -> Vec<ImportStatement> {
    let lines: Vec<&str> = source.lines().collect();
    let mut imports = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        if is_top_level_import(line, lang) {
            imports.push(ImportStatement {
                text: line.trim().to_string(),
                line: i + 1,
            });
        }
    }

    imports
}

/// Add import statements idempotently to source code.
///
/// Skips imports that already exist. Inserts at the appropriate position
/// (after existing imports, or after module doc comments).
pub fn add_imports(source: &str, imports_to_add: &[String], lang: Language) -> ImportsResult {
    let lines: Vec<&str> = source.lines().collect();
    let existing = list_imports(source, lang);
    let existing_texts: std::collections::HashSet<String> =
        existing.iter().map(|i| normalize_import(&i.text)).collect();

    let mut new_imports = Vec::new();
    for imp in imports_to_add {
        let normalized = normalize_import(imp);
        if !existing_texts.contains(&normalized) {
            new_imports.push(format_import(imp, lang));
        }
    }

    if new_imports.is_empty() {
        return ImportsResult {
            content: source.to_string(),
            added: 0,
            removed: 0,
            deduped: 0,
        };
    }

    let added = new_imports.len();

    // Find the insertion point: after the last existing import, or after
    // leading comments/doc attributes.
    let insert_after = find_import_insert_point(&lines, lang);

    let mut result = String::new();

    // None means "insert before the very first line"
    if insert_after.is_none() && !lines.is_empty() {
        for new_imp in &new_imports {
            result.push_str(new_imp);
            result.push('\n');
        }
        result.push('\n');
    }

    for (i, line) in lines.iter().enumerate() {
        result.push_str(line);
        result.push('\n');

        if insert_after == Some(i) {
            // If there were no imports before, add a blank line
            if existing.is_empty() && !line.is_empty() {
                result.push('\n');
            }
            for new_imp in &new_imports {
                result.push_str(new_imp);
                result.push('\n');
            }
        }
    }

    // Handle edge case: empty file
    if lines.is_empty() {
        for new_imp in &new_imports {
            result.push_str(new_imp);
            result.push('\n');
        }
    }

    // Preserve trailing newline behavior
    if !source.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    ImportsResult {
        content: result,
        added,
        removed: 0,
        deduped: 0,
    }
}

/// Remove specific import statements from source code.
pub fn remove_imports(source: &str, imports_to_remove: &[String], lang: Language) -> ImportsResult {
    let lines: Vec<&str> = source.lines().collect();
    let remove_set: std::collections::HashSet<String> = imports_to_remove
        .iter()
        .map(|i| normalize_import(i))
        .collect();

    let mut result = String::new();
    let mut removed = 0;

    for line in &lines {
        let trimmed = line.trim();
        if is_top_level_import(line, lang) && remove_set.contains(&normalize_import(trimmed)) {
            removed += 1;
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }

    // Preserve trailing newline behavior
    if !source.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    ImportsResult {
        content: result,
        added: 0,
        removed,
        deduped: 0,
    }
}

/// Deduplicate import statements in source code.
pub fn dedupe_imports(source: &str, lang: Language) -> ImportsResult {
    let lines: Vec<&str> = source.lines().collect();
    let mut seen = std::collections::HashSet::new();
    let mut result = String::new();
    let mut deduped = 0;

    for line in &lines {
        let trimmed = line.trim();
        if is_top_level_import(line, lang) {
            let normalized = normalize_import(trimmed);
            if seen.contains(&normalized) {
                deduped += 1;
                continue;
            }
            seen.insert(normalized);
        }
        result.push_str(line);
        result.push('\n');
    }

    // Preserve trailing newline behavior
    if !source.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    ImportsResult {
        content: result,
        added: 0,
        removed: 0,
        deduped,
    }
}

/// Check if a line is a top-level import (no leading whitespace).
///
/// Scoped imports inside function bodies, impl blocks, or modules are
/// indented and should not be treated as top-level imports.
fn is_top_level_import(line: &str, lang: Language) -> bool {
    let first = line.as_bytes().first().copied().unwrap_or(b' ');
    if first == b' ' || first == b'\t' {
        return false;
    }
    is_import_line(line.trim(), lang)
}

/// Check if a line is an import/use statement for the given language.
fn is_import_line(trimmed: &str, lang: Language) -> bool {
    match lang {
        Language::Rust => {
            (trimmed.starts_with("use ")
                || trimmed.starts_with("pub use ")
                || trimmed.starts_with("pub(crate) use ")
                || trimmed.starts_with("pub(super) use "))
                && trimmed.ends_with(';')
        }
        Language::Python => trimmed.starts_with("import ") || trimmed.starts_with("from "),
        Language::TypeScript | Language::JavaScript => trimmed.starts_with("import "),
        Language::Go => {
            trimmed.starts_with("import ") || (trimmed.starts_with('"') && trimmed.ends_with('"'))
        }
        Language::Java | Language::Kotlin => trimmed.starts_with("import "),
        _ => trimmed.starts_with("import ") || trimmed.starts_with("use "),
    }
}

/// Normalize an import statement for comparison (strip trailing semicolons, whitespace).
fn normalize_import(import: &str) -> String {
    import.trim().trim_end_matches(';').trim().to_string()
}

/// Format an import for insertion, ensuring it has proper syntax.
fn format_import(import: &str, lang: Language) -> String {
    let trimmed = import.trim();
    match lang {
        Language::Rust => {
            if trimmed.ends_with(';') {
                trimmed.to_string()
            } else {
                format!("{trimmed};")
            }
        }
        _ => trimmed.to_string(),
    }
}

/// Find the 0-based line index after which new imports should be inserted.
///
/// Returns `None` when imports should go before the very first line
/// (e.g. file starts directly with code, no leading comments).
fn find_import_insert_point(lines: &[&str], lang: Language) -> Option<usize> {
    // Strategy: find the last import line. If none, find the end of leading
    // comments/doc attributes.
    let mut last_import_line = None;

    for (i, line) in lines.iter().enumerate() {
        if is_top_level_import(line, lang) {
            last_import_line = Some(i);
        }
    }

    if let Some(idx) = last_import_line {
        return Some(idx);
    }

    // No existing imports: insert after leading comments, doc strings, shebang,
    // blank lines, and module-level attributes.
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with('#')
            || trimmed.starts_with("/*")
            || trimmed.starts_with('*')
            || trimmed.starts_with("*/")
            || trimmed.starts_with("\"\"\"")
            || trimmed.starts_with("'''")
        {
            continue;
        }
        // First non-comment, non-blank line: insert before it.
        // When i == 0 (file starts directly with code), return None
        // to signal "insert before the first line".
        return if i > 0 { Some(i - 1) } else { None };
    }

    // All comments/blank lines: insert at end
    Some(if lines.is_empty() { 0 } else { lines.len() - 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_rust_imports() {
        let source = "use std::io;\nuse std::fs;\n\nfn main() {}\n";
        let imports = list_imports(source, Language::Rust);
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].text, "use std::io;");
        assert_eq!(imports[0].line, 1);
        assert_eq!(imports[1].text, "use std::fs;");
    }

    #[test]
    fn add_rust_import() {
        let source = "use std::io;\n\nfn main() {}\n";
        let result = add_imports(source, &["use std::fs".into()], Language::Rust);
        assert_eq!(result.added, 1);
        assert!(result.content.contains("use std::fs;"));
        assert!(result.content.contains("use std::io;"));
    }

    #[test]
    fn add_import_idempotent() {
        let source = "use std::io;\n\nfn main() {}\n";
        let result = add_imports(source, &["use std::io;".into()], Language::Rust);
        assert_eq!(result.added, 0);
        assert_eq!(result.content, source);
    }

    #[test]
    fn remove_rust_import() {
        let source = "use std::io;\nuse std::fs;\n\nfn main() {}\n";
        let result = remove_imports(source, &["use std::io".into()], Language::Rust);
        assert_eq!(result.removed, 1);
        assert!(!result.content.contains("use std::io"));
        assert!(result.content.contains("use std::fs;"));
    }

    #[test]
    fn dedupe_rust_imports() {
        let source = "use std::io;\nuse std::fs;\nuse std::io;\n\nfn main() {}\n";
        let result = dedupe_imports(source, Language::Rust);
        assert_eq!(result.deduped, 1);
        let count = result.content.matches("use std::io;").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn add_python_import() {
        let source = "import os\n\ndef main():\n    pass\n";
        let result = add_imports(source, &["import sys".into()], Language::Python);
        assert_eq!(result.added, 1);
        assert!(result.content.contains("import sys"));
    }

    #[test]
    fn add_typescript_import() {
        let source = "import { foo } from 'bar';\n\nconst x = 1;\n";
        let result = add_imports(
            source,
            &["import { baz } from 'qux';".into()],
            Language::TypeScript,
        );
        assert_eq!(result.added, 1);
        assert!(result.content.contains("import { baz } from 'qux'"));
    }

    #[test]
    fn add_import_to_empty_file() {
        let source = "";
        let result = add_imports(source, &["use std::io;".into()], Language::Rust);
        assert_eq!(result.added, 1);
        assert!(result.content.contains("use std::io;"));
    }

    #[test]
    fn add_import_file_starts_with_code_no_comments() {
        // When a file starts directly with code (no comments, blanks, or
        // existing imports), the new import must be placed BEFORE the code.
        let source = "fn main() {}\n";
        let result = add_imports(source, &["use std::io".into()], Language::Rust);
        assert_eq!(result.added, 1);
        let lines: Vec<&str> = result.content.lines().collect();
        let import_pos = lines
            .iter()
            .position(|l| l.contains("use std::io"))
            .unwrap();
        let code_pos = lines.iter().position(|l| l.contains("fn main")).unwrap();
        assert!(
            import_pos < code_pos,
            "import (line {import_pos}) must appear before code (line {code_pos}): {:?}",
            lines,
        );
    }

    #[test]
    fn list_python_imports() {
        let source = "import os\nfrom pathlib import Path\n\ndef main():\n    pass\n";
        let imports = list_imports(source, Language::Python);
        assert_eq!(imports.len(), 2);
    }

    #[test]
    fn scoped_use_not_treated_as_top_level() {
        // Scoped `use` inside a function body should be ignored by all import functions
        let source =
            "use std::io;\n\nfn main() {\n    use std::fmt;\n    println!(\"hello\");\n}\n";
        let imports = list_imports(source, Language::Rust);
        assert_eq!(imports.len(), 1, "scoped use should not be listed");
        assert_eq!(imports[0].text, "use std::io;");
    }

    #[test]
    fn add_import_not_inside_function() {
        // New imports must be inserted at the top level, not inside a function body
        let source =
            "use std::io;\n\nfn main() {\n    use std::fmt;\n    println!(\"hello\");\n}\n";
        let result = add_imports(source, &["use std::fs".into()], Language::Rust);
        assert_eq!(result.added, 1);
        // The new import should appear after the existing top-level import (line 1),
        // not after the scoped import inside main()
        let lines: Vec<&str> = result.content.lines().collect();
        let fs_line = lines
            .iter()
            .position(|l| l.trim() == "use std::fs;")
            .expect("use std::fs; should be present");
        let fn_line = lines
            .iter()
            .position(|l| l.trim().starts_with("fn main"))
            .expect("fn main should be present");
        assert!(
            fs_line < fn_line,
            "new import at line {} should be before fn main at line {}",
            fs_line,
            fn_line
        );
    }

    #[test]
    fn remove_import_ignores_scoped_use() {
        // remove_imports should not remove scoped `use` inside function bodies
        let source =
            "use std::fmt;\n\nfn main() {\n    use std::fmt;\n    println!(\"hello\");\n}\n";
        let result = remove_imports(source, &["use std::fmt".into()], Language::Rust);
        assert_eq!(result.removed, 1);
        // The scoped `use std::fmt;` inside main() should remain
        assert!(
            result.content.contains("    use std::fmt;"),
            "scoped use should be preserved: {}",
            result.content
        );
    }

    #[test]
    fn dedupe_ignores_scoped_use() {
        // A scoped `use` that duplicates a top-level `use` should NOT be removed
        let source =
            "use std::fmt;\n\nfn main() {\n    use std::fmt;\n    println!(\"hello\");\n}\n";
        let result = dedupe_imports(source, Language::Rust);
        assert_eq!(result.deduped, 0, "scoped use is not a duplicate");
        assert_eq!(result.content, source);
    }
}
