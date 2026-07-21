//! AST-aware symbol rename: replace only identifier nodes, skipping
//! strings, comments, and documentation.

use std::path::Path;

use super::{Language, parse_source};

/// Node kinds that represent identifier tokens (rename targets).
const IDENTIFIER_KINDS: &[&str] = &[
    "identifier",
    "type_identifier",
    "field_identifier",
    "property_identifier",
    "simple_identifier",
    "shorthand_field_identifier",
    // Shell/Bash: function names and command names are `word` nodes
    "word",
];

/// Node kinds whose subtrees should be skipped entirely during rename.
///
/// Container kinds like `template_string`, `string`, and `concatenated_string`
/// are intentionally absent so the traversal enters them and finds identifiers
/// inside interpolation expressions (`${foo}` in JS/TS, `{foo}` in Python
/// f-strings, `#{foo}` in Ruby). The leaf text kinds below still prevent
/// renaming inside literal text fragments.
const SKIP_KINDS: &[&str] = &[
    "string_literal",
    "raw_string_literal",
    "string_content",
    "string_fragment",
    "line_comment",
    "block_comment",
    "comment",
    "doc_comment",
    // Python
    "string_start",
    "string_end",
];

/// Result of an AST-aware rename operation.
#[derive(Debug)]
pub struct RenameResult {
    /// The new file content after renaming.
    pub content: String,
    /// Number of replacements made.
    pub replacements: usize,
}

/// Rename all identifier occurrences of `old_name` to `new_name` in source code,
/// skipping strings and comments.
///
/// Returns `None` if the language has no grammar or parsing fails.
pub fn rename_in_source(
    source: &str,
    old_name: &str,
    new_name: &str,
    lang: Language,
) -> Option<RenameResult> {
    let (tree, _) = parse_source(source, lang)?;

    // Collect byte ranges to replace (in reverse order for offset stability)
    let mut replacements = Vec::new();
    collect_rename_nodes(tree.root_node(), source, old_name, &mut replacements);

    // Sort by start byte descending so we can replace from end to start
    replacements.sort_by_key(|&(start, _)| std::cmp::Reverse(start));

    let replacements_count = replacements.len();
    let mut result = source.to_string();
    for (start, end) in &replacements {
        result.replace_range(*start..*end, new_name);
    }

    Some(RenameResult {
        content: result,
        replacements: replacements_count,
    })
}

/// Rename identifiers in a file. Falls back to word-boundary replace if
/// tree-sitter cannot parse the language.
pub fn rename_in_file(
    path: &Path,
    old_name: &str,
    new_name: &str,
    lang_hint: Option<Language>,
) -> anyhow::Result<Option<RenameResult>> {
    let lang = lang_hint.unwrap_or_else(|| Language::from_path(path));
    // Strict sole-path (#1894): binary / invalid UTF-8 → InvalidInput.
    let source = crate::files::load_text_strict(path, &path.display().to_string())?;
    Ok(rename_in_source(&source, old_name, new_name, lang))
}

fn collect_rename_nodes(
    node: tree_sitter_lib::Node,
    source: &str,
    old_name: &str,
    results: &mut Vec<(usize, usize)>,
) {
    // Skip string/comment subtrees entirely
    if SKIP_KINDS.contains(&node.kind()) {
        return;
    }

    // Check if this is a matching identifier node
    if IDENTIFIER_KINDS.contains(&node.kind())
        && let Ok(text) = node.utf8_text(source.as_bytes())
        && text == old_name
    {
        results.push((node.start_byte(), node.end_byte()));
        return; // Leaf node, no children to visit
    }

    // Recurse into children
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            collect_rename_nodes(cursor.node(), source, old_name, results);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_rust_identifier_skips_strings() {
        let source = r#"
fn setup_file() -> &str {
    let name = "setup_file";
    // setup_file is important
    setup_file
}
"#;
        let result = rename_in_source(source, "setup_file", "init_file", Language::Rust).unwrap();
        // Should rename the function name and the trailing expression
        // but NOT the string literal or comment
        assert!(result.content.contains("fn init_file()"));
        assert!(result.content.contains("\"setup_file\"")); // string untouched
        assert!(result.content.contains("// setup_file")); // comment untouched
        assert!(result.replacements >= 2);
    }

    #[test]
    fn rename_rust_type_identifier() {
        let source = r#"
struct SetupFile {
    name: String,
}

impl SetupFile {
    fn new() -> SetupFile {
        SetupFile { name: String::new() }
    }
}
"#;
        let result = rename_in_source(source, "SetupFile", "ConfigFile", Language::Rust).unwrap();
        assert!(result.content.contains("struct ConfigFile"));
        assert!(result.content.contains("impl ConfigFile"));
        assert!(result.content.contains("-> ConfigFile"));
        assert!(!result.content.contains("SetupFile"));
    }

    #[test]
    fn rename_python_skips_strings_and_comments() {
        let source = r#"
def setup_file():
    """setup_file docs"""
    name = "setup_file"
    # setup_file comment
    return setup_file()
"#;
        let result = rename_in_source(source, "setup_file", "init_file", Language::Python).unwrap();
        assert!(result.content.contains("def init_file():"));
        // Strings and comments should be untouched
        assert!(result.content.contains("\"setup_file\""));
        assert!(result.content.contains("# setup_file"));
    }

    #[test]
    fn rename_no_matches_returns_zero() {
        let source = "fn main() {}\n";
        let result =
            rename_in_source(source, "nonexistent", "replacement", Language::Rust).unwrap();
        assert_eq!(result.replacements, 0);
        assert_eq!(result.content, source);
    }

    #[test]
    fn rename_unknown_language_returns_none() {
        let result = rename_in_source("anything", "x", "y", Language::Unknown);
        assert!(result.is_none());
    }

    #[test]
    fn rename_typescript_identifier_skips_strings() {
        let source = r#"
function processItem(item: string): string {
    const label = "processItem handler";
    // processItem does the work
    console.log(item);
    return processItem(item.trim());
}

class Worker {
    processItem(data: any): void {
        const result = processItem(data);
        console.log(result);
    }
}
"#;
        let result =
            rename_in_source(source, "processItem", "handleItem", Language::TypeScript).unwrap();
        // Function declaration and calls should be renamed
        assert!(result.content.contains("function handleItem("));
        assert!(result.content.contains("return handleItem("));
        // String literal and comment should NOT be renamed
        assert!(result.content.contains("\"processItem handler\""));
        assert!(result.content.contains("// processItem"));
        assert!(result.replacements >= 3);
    }

    #[test]
    fn rename_typescript_type_identifier() {
        let source = r#"
interface UserConfig {
    name: string;
    age: number;
}

function createConfig(): UserConfig {
    const cfg: UserConfig = { name: "test", age: 30 };
    return cfg;
}
"#;
        let result =
            rename_in_source(source, "UserConfig", "AppConfig", Language::TypeScript).unwrap();
        assert!(result.content.contains("interface AppConfig"));
        assert!(result.content.contains("): AppConfig"));
        assert!(result.content.contains("cfg: AppConfig"));
        assert!(!result.content.contains("UserConfig"));
    }

    #[test]
    fn rename_java_method_skips_strings() {
        let source = r#"
public class Service {
    public void processOrder(String id) {
        System.out.println("processOrder called");
        // processOrder handles business logic
        String result = processOrder(id);
    }

    private String processOrder(int count) {
        return "done";
    }
}
"#;
        let result =
            rename_in_source(source, "processOrder", "handleOrder", Language::Java).unwrap();
        // Method declarations and calls should be renamed
        assert!(result.content.contains("void handleOrder("));
        assert!(result.content.contains("String handleOrder("));
        // String literal and comment should NOT be renamed
        assert!(result.content.contains("\"processOrder called\""));
        assert!(result.content.contains("// processOrder"));
        assert!(result.replacements >= 3);
    }

    #[test]
    fn rename_typescript_template_string_interpolation() {
        let source = r#"
function greet(name: string): string {
    return `Hello, ${name}! Welcome ${name}.`;
}
"#;
        let result = rename_in_source(source, "name", "userName", Language::TypeScript).unwrap();
        // Identifiers inside ${} should be renamed
        assert!(result.content.contains("${userName}"));
        // But the function parameter should also be renamed
        assert!(result.content.contains("greet(userName:"));
        // The literal text "Hello, " should be untouched
        assert!(result.content.contains("Hello, "));
        assert!(result.replacements >= 3);
    }

    #[test]
    fn rename_python_fstring_interpolation() {
        let source = r#"
def greet(name):
    msg = "name is a label"
    return f"Hello, {name}!"
"#;
        let result = rename_in_source(source, "name", "user_name", Language::Python).unwrap();
        // The parameter and f-string interpolation should be renamed
        assert!(result.content.contains("def greet(user_name)"));
        assert!(result.content.contains("{user_name}"));
        // The regular string should NOT be renamed
        assert!(result.content.contains("\"name is a label\""));
    }

    #[test]
    fn rename_go_identifier() {
        let source = r#"
package main

func SetupFile() string {
    return "SetupFile"
}
"#;
        let result = rename_in_source(source, "SetupFile", "InitFile", Language::Go).unwrap();
        assert!(result.content.contains("func InitFile()"));
        assert!(result.content.contains("\"SetupFile\"")); // string untouched
    }

    /// Regression: bash/shell function names use `word` nodes in tree-sitter,
    /// not `identifier`. Without `word` in `IDENTIFIER_KINDS`, ast rename
    /// found 0 matches even though the function existed, and fell through
    /// to the "tree-sitter parsed successfully but 0 matches" error path.
    #[test]
    fn rename_bash_function() {
        let source = r#"#!/usr/bin/env bash
build_image() {
    docker build -t app .
}
push_image() {
    docker push app
}
main() {
    build_image
    push_image
}
"#;
        let result = rename_in_source(source, "build_image", "build_container", Language::Shell)
            .expect("shell parse should succeed");
        assert!(
            result.content.contains("build_container()"),
            "function definition should be renamed: {}",
            result.content
        );
        assert!(
            result.content.contains("    build_container"),
            "function call should be renamed: {}",
            result.content
        );
        assert!(
            !result.content.contains("build_image"),
            "old name should be gone: {}",
            result.content
        );
        assert!(
            result.replacements >= 2,
            "should rename definition + call site, got {}",
            result.replacements
        );
    }

    #[test]
    fn rename_returns_zero_when_name_only_in_strings_and_comments() {
        // #1187: tree-sitter parses successfully but name only appears in
        // strings/comments, so replacements should be 0 (not None).
        let source = r#"
fn main() {
    // target is mentioned here
    let s = "target";
    println!("{}", s);
}
"#;
        let result = rename_in_source(source, "target", "renamed", Language::Rust).unwrap();
        assert_eq!(
            result.replacements, 0,
            "should be 0 when name is only in strings/comments"
        );
        // Source should be unchanged
        assert_eq!(result.content, source);
    }
}
