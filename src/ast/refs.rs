//! Find references to a symbol across files.

use std::path::Path;

use serde::Serialize;

use super::{Language, parse_source};

/// Kind of reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    /// The definition site.
    Definition,
    /// A usage/call site.
    Reference,
}

/// A single reference to a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct SymbolRef {
    /// File path (relative).
    pub file: String,
    /// 1-based line number.
    pub line: usize,
    /// The source line containing the reference (trimmed).
    pub context: String,
    /// Whether this is a definition or reference.
    pub kind: RefKind,
}

/// Node kinds that typically indicate a definition context.
const DEFINITION_PARENT_KINDS: &[&str] = &[
    "function_item",
    "function_definition",
    "function_declaration",
    "method_declaration",
    "method_definition",
    "struct_item",
    "struct_declaration",
    "enum_item",
    "enum_declaration",
    "trait_item",
    "trait_declaration",
    "impl_item",
    "class_definition",
    "class_declaration",
    "interface_declaration",
    "type_declaration",
    "type_item",
    "mod_item",
    "const_item",
    "type_spec",
];

/// Identifier node kinds to scan.
const IDENTIFIER_KINDS: &[&str] = &[
    "identifier",
    "type_identifier",
    "field_identifier",
    "property_identifier",
    "simple_identifier",
];

/// Node kinds whose subtrees should be skipped (strings, comments).
const SKIP_KINDS: &[&str] = &[
    "string_literal",
    "raw_string_literal",
    "string",
    "string_content",
    "string_fragment",
    "template_string",
    "line_comment",
    "block_comment",
    "comment",
    "doc_comment",
];

/// Find all references to `symbol_name` in the given source code.
pub fn find_refs_in_source(
    source: &str,
    symbol_name: &str,
    lang: Language,
    file_path: &str,
) -> Vec<SymbolRef> {
    let Some((tree, _)) = parse_source(source, lang) else {
        return Vec::new();
    };
    find_refs_in_source_with_tree(source, symbol_name, &tree, file_path)
}

/// Find all references using a pre-parsed tree (avoids redundant parsing).
///
/// Use this when you already have a [`tree_sitter_lib::Tree`] for the source,
/// e.g. from a cache keyed by file path. This eliminates the O(N) re-parsing
/// cost when scanning the same file for multiple symbol names.
pub fn find_refs_in_source_with_tree(
    source: &str,
    symbol_name: &str,
    tree: &tree_sitter_lib::Tree,
    file_path: &str,
) -> Vec<SymbolRef> {
    let lines: Vec<&str> = source.lines().collect();
    let mut refs = Vec::new();
    collect_refs(
        tree.root_node(),
        source,
        symbol_name,
        &lines,
        file_path,
        &mut refs,
    );
    refs
}

/// Find ALL identifier references in source (unfiltered by name).
///
/// Returns `(identifier_name, SymbolRef)` pairs for every identifier node
/// in the AST. Used by impact analysis to build a reverse dependency map
/// in a single pass instead of re-scanning per symbol.
pub fn find_all_refs_in_source_with_tree(
    source: &str,
    tree: &tree_sitter_lib::Tree,
    file_path: &str,
) -> Vec<(String, SymbolRef)> {
    let lines: Vec<&str> = source.lines().collect();
    let mut refs = Vec::new();
    collect_all_refs(tree.root_node(), source, &lines, file_path, &mut refs);
    refs
}

/// Find refs across multiple files.
pub fn find_refs_in_file(
    path: &Path,
    symbol_name: &str,
    lang_hint: Option<Language>,
    display_path: &str,
) -> Vec<SymbolRef> {
    let lang = lang_hint.unwrap_or_else(|| Language::from_path(path));
    if !lang.has_grammar() {
        return Vec::new();
    }
    let Ok(source) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    find_refs_in_source(&source, symbol_name, lang, display_path)
}

fn collect_refs(
    node: tree_sitter_lib::Node,
    source: &str,
    symbol_name: &str,
    lines: &[&str],
    file_path: &str,
    refs: &mut Vec<SymbolRef>,
) {
    if SKIP_KINDS.contains(&node.kind()) {
        return;
    }

    if IDENTIFIER_KINDS.contains(&node.kind())
        && let Ok(text) = node.utf8_text(source.as_bytes())
        && text == symbol_name
    {
        let line = node.start_position().row + 1;
        let context = lines
            .get(node.start_position().row)
            .unwrap_or(&"")
            .trim()
            .to_string();

        let kind = if is_definition_site(node) {
            RefKind::Definition
        } else {
            RefKind::Reference
        };

        refs.push(SymbolRef {
            file: file_path.to_string(),
            line,
            context,
            kind,
        });
    }

    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            collect_refs(cursor.node(), source, symbol_name, lines, file_path, refs);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

fn collect_all_refs(
    node: tree_sitter_lib::Node,
    source: &str,
    lines: &[&str],
    file_path: &str,
    refs: &mut Vec<(String, SymbolRef)>,
) {
    if SKIP_KINDS.contains(&node.kind()) {
        return;
    }

    if IDENTIFIER_KINDS.contains(&node.kind())
        && let Ok(text) = node.utf8_text(source.as_bytes())
    {
        let line = node.start_position().row + 1;
        let context = lines
            .get(node.start_position().row)
            .unwrap_or(&"")
            .trim()
            .to_string();

        let kind = if is_definition_site(node) {
            RefKind::Definition
        } else {
            RefKind::Reference
        };

        refs.push((
            text.to_string(),
            SymbolRef {
                file: file_path.to_string(),
                line,
                context,
                kind,
            },
        ));
    }

    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            collect_all_refs(cursor.node(), source, lines, file_path, refs);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

/// Check if this identifier node is in a definition position.
fn is_definition_site(node: tree_sitter_lib::Node) -> bool {
    // Walk up parents to see if we're directly inside a definition node
    // where this identifier is the "name" child
    if let Some(parent) = node.parent()
        && DEFINITION_PARENT_KINDS.contains(&parent.kind())
    {
        // Use tree-sitter's field API: the "name" field points to the
        // symbol being defined. Other identifier children (parameters,
        // return types, etc.) are NOT definitions.
        if let Some(name_node) = parent.child_by_field_name("name") {
            return name_node.id() == node.id();
        }
        // Fallback for grammars without a "name" field: treat the first
        // identifier child as the definition name.
        let mut cursor = parent.walk();
        for child in parent.children(&mut cursor) {
            if IDENTIFIER_KINDS.contains(&child.kind()) {
                return child.id() == node.id();
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_refs_rust() {
        let source = r#"
fn setup_file() -> String {
    String::new()
}

fn main() {
    let x = setup_file();
    let y = setup_file();
}
"#;
        let refs = find_refs_in_source(source, "setup_file", Language::Rust, "test.rs");
        assert!(refs.len() >= 3); // 1 def + 2 refs
        let defs: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == RefKind::Definition)
            .collect();
        let uses: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == RefKind::Reference)
            .collect();
        assert!(!defs.is_empty());
        assert!(uses.len() >= 2);
    }

    #[test]
    fn find_refs_skips_strings_and_comments() {
        let source = r#"
fn target() {}

fn main() {
    // target is important
    let s = "target";
    target();
}
"#;
        let refs = find_refs_in_source(source, "target", Language::Rust, "test.rs");
        // Should find def + call, but NOT string or comment
        for r in &refs {
            assert!(!r.context.starts_with("//"));
            assert!(!r.context.contains("\"target\""));
        }
    }

    #[test]
    fn find_refs_python() {
        let source = r#"
def helper():
    pass

def main():
    helper()
    helper()
"#;
        let refs = find_refs_in_source(source, "helper", Language::Python, "test.py");
        assert!(refs.len() >= 3);
    }

    #[test]
    fn find_refs_typescript() {
        let source = r#"
function calculate(x: number): number {
    return x * 2;
}

function main(): void {
    const a = calculate(10);
    const b = calculate(20);
    console.log("calculate result", a + b);
}

class MathHelper {
    run(): number {
        return calculate(42);
    }
}
"#;
        let refs = find_refs_in_source(source, "calculate", Language::TypeScript, "test.ts");
        // Should find def + 3 call-site refs, NOT the string "calculate result"
        let defs: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == RefKind::Definition)
            .collect();
        let uses: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == RefKind::Reference)
            .collect();
        assert!(!defs.is_empty(), "should find at least one definition");
        assert!(uses.len() >= 3, "should find at least 3 reference sites");
        // No ref should come from the string literal line
        for r in &refs {
            assert!(
                !r.context.contains("\"calculate"),
                "string literal should be skipped"
            );
        }
    }

    #[test]
    fn find_refs_java() {
        let source = r#"
public class OrderService {
    public void processOrder(String id) {
        System.out.println("processOrder: " + id);
    }

    public void batchProcess() {
        processOrder("abc");
        processOrder("def");
    }
}
"#;
        let refs = find_refs_in_source(source, "processOrder", Language::Java, "OrderService.java");
        // Should find the method def + 2 calls; NOT the string
        let defs: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == RefKind::Definition)
            .collect();
        let uses: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == RefKind::Reference)
            .collect();
        assert!(!defs.is_empty(), "should find processOrder definition");
        assert!(
            uses.len() >= 2,
            "should find at least 2 processOrder call refs"
        );
        // String should not appear as a ref
        for r in &refs {
            assert!(
                !r.context.contains("\"processOrder:"),
                "string literal should be skipped"
            );
        }
    }

    #[test]
    fn find_refs_c() {
        let source = r#"
#include <stdio.h>

void helper(int x) {
    printf("%d\n", x);
}

int main() {
    helper(1);
    helper(2);
    // helper is useful
    return 0;
}
"#;
        let refs = find_refs_in_source(source, "helper", Language::C, "main.c");
        // Should find the declaration + at least 2 call sites
        // (C grammar nests identifiers inside function_declarator,
        // so definition detection may classify the decl as Reference)
        assert!(
            refs.len() >= 3,
            "should find at least 3 occurrences of helper, got {}",
            refs.len()
        );
        // Comment should be skipped
        for r in &refs {
            assert!(!r.context.starts_with("//"), "comment should not be a ref");
        }
    }

    #[test]
    fn unknown_language_returns_empty() {
        let refs = find_refs_in_source("anything", "x", Language::Unknown, "test.txt");
        assert!(refs.is_empty());
    }

    /// Verify that `find_refs_in_source_with_tree` produces the same results
    /// as `find_refs_in_source` when given a pre-parsed tree (regression guard
    /// for the tree-cache optimization added in #934).
    #[test]
    fn find_refs_with_tree_matches_full_parse() {
        let source = "fn greet() {}\nfn main() { greet(); greet(); }\n";
        let lang = Language::Rust;
        let (tree, _) = crate::ast::parse_source(source, lang).expect("parse should succeed");
        let via_full = find_refs_in_source(source, "greet", lang, "test.rs");
        let via_tree = find_refs_in_source_with_tree(source, "greet", &tree, "test.rs");
        assert_eq!(
            via_full.len(),
            via_tree.len(),
            "with_tree should produce the same number of refs"
        );
        for (a, b) in via_full.iter().zip(via_tree.iter()) {
            assert_eq!(a.line, b.line);
            assert_eq!(a.kind, b.kind);
            assert_eq!(a.context, b.context);
        }
    }

    #[test]
    fn find_all_refs_returns_all_identifiers() {
        let source = "fn foo() {}\nfn bar() { foo(); }\n";
        let lang = Language::Rust;
        let (tree, _) = crate::ast::parse_source(source, lang).expect("parse");
        let all = find_all_refs_in_source_with_tree(source, &tree, "test.rs");
        // Should contain both "foo" and "bar" identifiers
        let names: Vec<&str> = all.iter().map(|(name, _)| name.as_str()).collect();
        assert!(names.contains(&"foo"), "should contain foo, got {names:?}");
        assert!(names.contains(&"bar"), "should contain bar, got {names:?}");
        // "foo" should appear at least twice (def + ref from bar)
        let foo_count = names.iter().filter(|n| **n == "foo").count();
        assert!(
            foo_count >= 2,
            "foo should appear at least twice (def + ref), got {foo_count}"
        );
    }

    #[test]
    fn ref_kind_serializes() {
        let json = serde_json::to_string(&RefKind::Definition).unwrap();
        assert_eq!(json, "\"definition\"");
    }

    #[test]
    fn is_definition_site_only_matches_name_child() {
        // Parameters inside a function definition should NOT be classified
        // as definitions of the parameter name.
        let source = r#"
fn process(input: &str, count: usize) -> bool {
    count > 0
}

fn main() {
    process("hello", 5);
}
"#;
        let refs = find_refs_in_source(source, "process", Language::Rust, "test.rs");
        let defs: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == RefKind::Definition)
            .collect();
        // "process" should have exactly one definition (the fn declaration)
        assert_eq!(
            defs.len(),
            1,
            "expected exactly 1 definition of 'process', got {defs:?}"
        );

        // "input" should NOT be classified as a definition (it's a parameter)
        let input_refs = find_refs_in_source(source, "input", Language::Rust, "test.rs");
        let input_defs: Vec<_> = input_refs
            .iter()
            .filter(|r| r.kind == RefKind::Definition)
            .collect();
        // Parameters are not definitions of top-level symbols
        assert!(
            input_defs.is_empty(),
            "parameter 'input' should not be classified as a definition: {input_defs:?}"
        );
    }
}
