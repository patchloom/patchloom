//! AST-aware operations using tree-sitter grammars.
//!
//! This module provides language detection, source file parsing, symbol
//! extraction, AST-aware rename, and syntax validation for 20 languages.
//! All functionality is gated on the `ast` feature flag.

pub mod deps;
pub mod diff;
pub mod impact;
pub mod map;
pub mod refs;
pub mod rename;
pub mod replace;
pub mod search;
pub mod symbols;
pub mod validate;

use std::path::Path;

use serde::{Deserialize, Serialize};

/// A programming, markup, or data language detected by file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    Java,
    CSharp,
    Ruby,
    Php,
    Swift,
    Kotlin,
    Cpp,
    C,
    Hcl,
    Xml,
    Protobuf,
    Dockerfile,
    Markdown,
    Toml,
    Yaml,
    Json,
    Shell,
    Unknown,
}

impl Language {
    /// Detect language from a file extension string (without the leading dot).
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rs" => Self::Rust,
            "ts" | "tsx" => Self::TypeScript,
            "js" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
            "py" | "pyi" => Self::Python,
            "go" => Self::Go,
            "java" => Self::Java,
            "cs" => Self::CSharp,
            "rb" => Self::Ruby,
            "php" => Self::Php,
            "swift" => Self::Swift,
            "kt" | "kts" => Self::Kotlin,
            "c" | "h" => Self::C,
            "cpp" | "cxx" | "cc" | "hpp" | "hxx" => Self::Cpp,
            "hcl" | "tf" | "tfvars" => Self::Hcl,
            "xml" | "xsl" | "xslt" | "xsd" | "svg" | "plist" => Self::Xml,
            "proto" => Self::Protobuf,
            "dockerfile" => Self::Dockerfile,
            "md" | "mdx" => Self::Markdown,
            "toml" => Self::Toml,
            "yml" | "yaml" => Self::Yaml,
            "json" => Self::Json,
            "sh" | "bash" | "zsh" => Self::Shell,
            _ => Self::Unknown,
        }
    }

    /// Detect language from a file path by its extension.
    pub fn from_path(path: &Path) -> Self {
        // Handle extensionless files by filename.
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            let lower = name.to_lowercase();
            if lower == "dockerfile" || lower.starts_with("dockerfile.") {
                return Self::Dockerfile;
            }
            if lower == "makefile" || lower == "gnumakefile" {
                return Self::Shell; // Makefiles use shell syntax
            }
        }
        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => Self::from_extension(ext),
            None => Self::Unknown,
        }
    }

    /// Returns `true` if this language has tree-sitter grammar support.
    pub fn has_grammar(self) -> bool {
        !matches!(self, Self::Markdown | Self::Dockerfile | Self::Unknown)
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Rust => "Rust",
            Self::TypeScript => "TypeScript",
            Self::JavaScript => "JavaScript",
            Self::Python => "Python",
            Self::Go => "Go",
            Self::Java => "Java",
            Self::CSharp => "C#",
            Self::Ruby => "Ruby",
            Self::Php => "PHP",
            Self::Swift => "Swift",
            Self::Kotlin => "Kotlin",
            Self::Cpp => "C++",
            Self::C => "C",
            Self::Hcl => "HCL",
            Self::Xml => "XML",
            Self::Protobuf => "Protobuf",
            Self::Dockerfile => "Dockerfile",
            Self::Markdown => "Markdown",
            Self::Toml => "TOML",
            Self::Yaml => "YAML",
            Self::Json => "JSON",
            Self::Shell => "Shell",
            Self::Unknown => "Unknown",
        };
        f.write_str(s)
    }
}

/// Map a [`Language`] to its tree-sitter grammar.
pub(crate) fn ts_language_for(lang: Language) -> Option<tree_sitter_lib::Language> {
    match lang {
        Language::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
        Language::Python => Some(tree_sitter_python::LANGUAGE.into()),
        Language::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        Language::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
        Language::Go => Some(tree_sitter_go::LANGUAGE.into()),
        Language::Shell => Some(tree_sitter_bash::LANGUAGE.into()),
        Language::Hcl => Some(tree_sitter_hcl::LANGUAGE.into()),
        Language::Toml => Some(tree_sitter_toml_ng::LANGUAGE.into()),
        Language::Yaml => Some(tree_sitter_yaml::LANGUAGE.into()),
        Language::Json => Some(tree_sitter_json::LANGUAGE.into()),
        Language::Xml => Some(tree_sitter_xml::LANGUAGE_XML.into()),
        Language::Protobuf => Some(tree_sitter_proto::LANGUAGE.into()),
        Language::C => Some(tree_sitter_c::LANGUAGE.into()),
        Language::Cpp => Some(tree_sitter_cpp::LANGUAGE.into()),
        Language::Java => Some(tree_sitter_java::LANGUAGE.into()),
        Language::Ruby => Some(tree_sitter_ruby::LANGUAGE.into()),
        Language::CSharp => Some(tree_sitter_c_sharp::LANGUAGE.into()),
        Language::Swift => Some(tree_sitter_swift::LANGUAGE.into()),
        Language::Kotlin => Some(tree_sitter_kotlin_sg::LANGUAGE.into()),
        Language::Php => Some(tree_sitter_php::LANGUAGE_PHP.into()),
        _ => None,
    }
}

/// Parse source text for a given language, returning the tree-sitter tree.
pub(crate) fn parse_source(
    source: &str,
    lang: Language,
) -> Option<(tree_sitter_lib::Tree, tree_sitter_lib::Language)> {
    let ts_lang = ts_language_for(lang)?;
    let mut parser = tree_sitter_lib::Parser::new();
    parser.set_language(&ts_lang).ok()?;
    let tree = parser.parse(source, None)?;
    Some((tree, ts_lang))
}

/// Find the text of the first child with a given node kind.
pub(crate) fn child_text_by_kind<'a>(
    node: tree_sitter_lib::Node<'a>,
    kind: &str,
    source: &'a str,
) -> Option<&'a str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            return child.utf8_text(source.as_bytes()).ok();
        }
    }
    None
}

/// Find the text of the first child matching any of the given kinds.
pub(crate) fn child_text_by_kinds<'a>(
    node: tree_sitter_lib::Node<'a>,
    kinds: &[&str],
    source: &'a str,
) -> Option<&'a str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if kinds.contains(&child.kind()) {
            return child.utf8_text(source.as_bytes()).ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn language_from_extension() {
        assert_eq!(Language::from_extension("rs"), Language::Rust);
        assert_eq!(Language::from_extension("ts"), Language::TypeScript);
        assert_eq!(Language::from_extension("tsx"), Language::TypeScript);
        assert_eq!(Language::from_extension("js"), Language::JavaScript);
        assert_eq!(Language::from_extension("py"), Language::Python);
        assert_eq!(Language::from_extension("go"), Language::Go);
        assert_eq!(Language::from_extension("java"), Language::Java);
        assert_eq!(Language::from_extension("cs"), Language::CSharp);
        assert_eq!(Language::from_extension("rb"), Language::Ruby);
        assert_eq!(Language::from_extension("php"), Language::Php);
        assert_eq!(Language::from_extension("swift"), Language::Swift);
        assert_eq!(Language::from_extension("kt"), Language::Kotlin);
        assert_eq!(Language::from_extension("c"), Language::C);
        assert_eq!(Language::from_extension("cpp"), Language::Cpp);
        assert_eq!(Language::from_extension("hcl"), Language::Hcl);
        assert_eq!(Language::from_extension("tf"), Language::Hcl);
        assert_eq!(Language::from_extension("proto"), Language::Protobuf);
        assert_eq!(Language::from_extension("sh"), Language::Shell);
        assert_eq!(Language::from_extension("toml"), Language::Toml);
        assert_eq!(Language::from_extension("yml"), Language::Yaml);
        assert_eq!(Language::from_extension("json"), Language::Json);
        assert_eq!(Language::from_extension("xml"), Language::Xml);
        assert_eq!(Language::from_extension("unknown"), Language::Unknown);
    }

    #[test]
    fn language_from_extension_case_insensitive() {
        assert_eq!(Language::from_extension("RS"), Language::Rust);
        assert_eq!(Language::from_extension("Py"), Language::Python);
    }

    #[test]
    fn language_from_path_uses_extension() {
        assert_eq!(
            Language::from_path(Path::new("src/main.rs")),
            Language::Rust
        );
        assert_eq!(
            Language::from_path(Path::new("lib/foo.py")),
            Language::Python
        );
    }

    #[test]
    fn language_from_path_dockerfile() {
        assert_eq!(
            Language::from_path(Path::new("Dockerfile")),
            Language::Dockerfile
        );
        assert_eq!(
            Language::from_path(Path::new("Dockerfile.prod")),
            Language::Dockerfile
        );
    }

    #[test]
    fn has_grammar_excludes_non_parseable() {
        assert!(Language::Rust.has_grammar());
        assert!(Language::Python.has_grammar());
        assert!(!Language::Markdown.has_grammar());
        assert!(!Language::Dockerfile.has_grammar());
        assert!(!Language::Unknown.has_grammar());
    }

    #[test]
    fn parse_source_rust() {
        let source = "fn main() { println!(\"hello\"); }";
        let (tree, _) = parse_source(source, Language::Rust).expect("should parse Rust source");
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn parse_source_python() {
        let source = "def hello():\n    print('hello')\n";
        let (tree, _) = parse_source(source, Language::Python).expect("should parse Python source");
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn parse_source_unknown_returns_none() {
        let result = parse_source("anything", Language::Unknown);
        assert!(result.is_none());
    }

    #[test]
    fn display_formatting() {
        assert_eq!(Language::Rust.to_string(), "Rust");
        assert_eq!(Language::CSharp.to_string(), "C#");
        assert_eq!(Language::Cpp.to_string(), "C++");
    }
}
