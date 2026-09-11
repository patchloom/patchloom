//! AST-aware operations using tree-sitter grammars.
//!
//! This module provides language detection, source file parsing, symbol
//! extraction, AST-aware rename, and syntax validation for 20 languages.
//! All functionality is gated on the `ast` feature flag.

pub mod deps;
pub mod diff;
pub mod extract_to_file;
pub mod group;
pub mod impact;
pub mod import_rewrite;
pub mod imports;
pub mod insert;
pub mod map;
pub mod move_symbols;
pub mod refs;
pub mod rename;
pub mod reorder;
pub mod replace;
pub mod rewrite;

pub mod search;
pub mod split;
pub mod symbol_extract;
pub mod symbols;
pub mod validate;
pub mod wrap;

#[cfg(test)]
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::ControlFlow;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

thread_local! {
    static PARSERS: RefCell<HashMap<Language, tree_sitter_lib::Parser>> =
        RefCell::new(HashMap::new());
}

/// Five seconds is well above a typical file (a 1 MB generated source
/// is usually tens of milliseconds) and still bounds MCP
/// `spawn_blocking` threads on pathological input (#2384).
const PARSE_TIMEOUT: Duration = Duration::from_millis(5_000);

#[cfg(test)]
thread_local! {
    static PARSE_TIMEOUT_OVERRIDE: Cell<Option<Duration>> = const { Cell::new(None) };
}

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

    /// Detect language from a language name or file extension string.
    /// Tries common language names first (e.g. "rust", "python",
    /// "typescript"), then falls back to extension matching.
    pub fn from_name_or_ext(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "rust" => Self::Rust,
            "typescript" => Self::TypeScript,
            "javascript" => Self::JavaScript,
            "python" => Self::Python,
            "golang" => Self::Go,
            "java" => Self::Java,
            "csharp" | "c#" => Self::CSharp,
            "ruby" => Self::Ruby,
            "kotlin" => Self::Kotlin,
            "hcl" | "terraform" => Self::Hcl,
            "protobuf" => Self::Protobuf,
            "dockerfile" | "docker" => Self::Dockerfile,
            "markdown" => Self::Markdown,
            "c++" => Self::Cpp,
            "shell" => Self::Shell,
            _ => Self::from_extension(s),
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
///
/// Returns the tree-sitter `Language` object for supported languages, or
/// `None` for languages without grammar support (Markdown, Dockerfile, Unknown).
///
/// Library consumers can use this to build custom tree-sitter parsers using
/// the same grammar versions that patchloom uses internally.
pub fn ts_language_for(lang: Language) -> Option<tree_sitter_lib::Language> {
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

/// Why [`try_parse_source`] did not return a tree (#2406).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseFailure {
    /// Language has no tree-sitter grammar, or the parser could not be set up.
    NoGrammar,
    /// The 5 second parse deadline fired before a tree was produced.
    DeadlineExceeded,
}

/// Parse source text, distinguishing no-grammar from a deadline (#2406).
///
/// [`parse_source`] stays an `Option` wrapper for existing callers.
pub fn try_parse_source(
    source: &str,
    lang: Language,
) -> Result<(tree_sitter_lib::Tree, tree_sitter_lib::Language), ParseFailure> {
    let ts_lang = ts_language_for(lang).ok_or(ParseFailure::NoGrammar)?;
    let tree = PARSERS.with(|slot| {
        let mut map = slot.borrow_mut();
        let parser = match map.entry(lang) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(v) => {
                let mut parser = tree_sitter_lib::Parser::new();
                parser
                    .set_language(&ts_lang)
                    .map_err(|_| ParseFailure::NoGrammar)?;
                v.insert(parser)
            }
        };
        // Resume after a cancelled parse would continue mid-document.
        parser.reset();
        match parse_with_deadline(parser, source) {
            Ok(tree) => Ok(tree),
            Err(e) => {
                parser.reset();
                Err(e)
            }
        }
    })?;
    Ok((tree, ts_lang))
}

/// Parse source text for a given language, returning the tree-sitter tree.
///
/// Reuses a thread-local [`tree_sitter_lib::Parser`] per [`Language`].
/// Returns `None` if the language has no grammar support, if parsing
/// fails, or if the parse exceeds the 5 second deadline.
///
/// # Example
///
/// ```rust
/// use patchloom::ast::{parse_source, Language};
///
/// let source = "fn main() { println!(\"hello\"); }";
/// let (tree, _lang) = parse_source(source, Language::Rust).unwrap();
/// assert!(!tree.root_node().has_error());
/// ```
pub fn parse_source(
    source: &str,
    lang: Language,
) -> Option<(tree_sitter_lib::Tree, tree_sitter_lib::Language)> {
    try_parse_source(source, lang).ok()
}

fn parse_deadline() -> Duration {
    #[cfg(test)]
    {
        if let Some(d) = PARSE_TIMEOUT_OVERRIDE.with(Cell::get) {
            return d;
        }
    }
    PARSE_TIMEOUT
}

fn parse_with_deadline(
    parser: &mut tree_sitter_lib::Parser,
    source: &str,
) -> Result<tree_sitter_lib::Tree, ParseFailure> {
    let deadline = Instant::now() + parse_deadline();
    let timed_out = std::cell::Cell::new(false);
    let mut progress = |_state: &tree_sitter_lib::ParseState| {
        if Instant::now() >= deadline {
            timed_out.set(true);
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let options = tree_sitter_lib::ParseOptions::new().progress_callback(&mut progress);
    let bytes = source.as_bytes();
    let len = bytes.len();
    match parser.parse_with_options(
        &mut |i, _| {
            if i < len { &bytes[i..] } else { &[] as &[u8] }
        },
        None,
        Some(options),
    ) {
        Some(tree) => Ok(tree),
        None if timed_out.get() => Err(ParseFailure::DeadlineExceeded),
        None => Err(ParseFailure::NoGrammar),
    }
}

/// Override the per-file parse deadline in tests (`cfg(test)` only).
#[cfg(test)]
pub(crate) struct ParseTimeoutGuard {
    prev: Option<Duration>,
}

#[cfg(test)]
impl ParseTimeoutGuard {
    pub(crate) fn set(timeout: Duration) -> Self {
        let prev = PARSE_TIMEOUT_OVERRIDE.with(|c| c.replace(Some(timeout)));
        Self { prev }
    }
}

#[cfg(test)]
impl Drop for ParseTimeoutGuard {
    fn drop(&mut self) {
        PARSE_TIMEOUT_OVERRIDE.with(|c| c.set(self.prev));
    }
}

/// Find the text of the first child with a given node kind.
///
/// Walks the immediate children of `node` and returns the source text
/// of the first child whose `kind()` matches `kind`. Useful for building
/// custom AST extractors on top of patchloom's tree-sitter grammars.
pub fn child_text_by_kind<'a>(
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
///
/// Like [`child_text_by_kind`], but matches against multiple node kinds.
/// Returns the source text of the first child whose kind is in `kinds`.
pub fn child_text_by_kinds<'a>(
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
    fn try_parse_source_unknown_is_no_grammar() {
        assert_eq!(
            try_parse_source("anything", Language::Unknown).unwrap_err(),
            ParseFailure::NoGrammar
        );
    }

    #[test]
    fn try_parse_source_deadline_is_distinct() {
        let _guard = ParseTimeoutGuard::set(Duration::from_millis(1));
        let source = nested_rust_source(80_000);
        assert_eq!(
            try_parse_source(&source, Language::Rust).unwrap_err(),
            ParseFailure::DeadlineExceeded
        );
    }

    fn nested_rust_source(depth: usize) -> String {
        let mut source = String::from("fn main() { let x = ");
        source.push_str(&"(".repeat(depth));
        source.push('1');
        source.push_str(&")".repeat(depth));
        source.push_str("; }\n");
        source
    }

    #[test]
    fn parse_source_pathological_returns_none() {
        let _guard = ParseTimeoutGuard::set(Duration::from_millis(1));
        let source = nested_rust_source(80_000);
        let start = Instant::now();
        assert!(
            parse_source(&source, Language::Rust).is_none(),
            "deeply nested source must take the existing None path"
        );
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "deadline must cancel instead of hanging"
        );
    }

    #[test]
    fn parse_source_still_parses_after_timeout() {
        {
            let _guard = ParseTimeoutGuard::set(Duration::from_millis(1));
            let source = nested_rust_source(80_000);
            assert!(parse_source(&source, Language::Rust).is_none());
        }
        let source = "fn main() { println!(\"hello\"); }";
        let (tree, _) = parse_source(source, Language::Rust).expect("reset after timeout");
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn parse_source_reuses_parser_across_calls() {
        let rust = "fn main() {}";
        let python = "def hello():\n    pass\n";
        let (a, _) = parse_source(rust, Language::Rust).expect("first rust");
        let (b, _) = parse_source(python, Language::Python).expect("python");
        PARSERS.with(|slot| {
            let map = slot.borrow();
            assert!(map.contains_key(&Language::Rust));
            assert!(map.contains_key(&Language::Python));
        });
        let cached = PARSERS.with(|slot| slot.borrow().len());
        let (c, _) = parse_source(rust, Language::Rust).expect("second rust");
        let cached_again = PARSERS.with(|slot| slot.borrow().len());
        assert_eq!(
            cached_again, cached,
            "second rust parse must reuse the cache"
        );
        assert!(!a.root_node().has_error());
        assert!(!b.root_node().has_error());
        assert!(!c.root_node().has_error());
    }

    #[test]
    fn default_grammars_load_on_tree_sitter_027() {
        // tree-sitter 0.27 can reject older language ABIs at set_language.
        // Exhaustive so a new Language variant must be classified here.
        use Language::*;
        let langs = [
            Rust, TypeScript, JavaScript, Python, Go, Java, CSharp, Ruby, Php, Swift, Kotlin, Cpp,
            C, Hcl, Xml, Protobuf, Dockerfile, Markdown, Toml, Yaml, Json, Shell, Unknown,
        ];
        let _exhaust = |l: Language| match l {
            Rust | TypeScript | JavaScript | Python | Go | Java | CSharp | Ruby | Php | Swift
            | Kotlin | Cpp | C | Hcl | Xml | Protobuf | Dockerfile | Markdown | Toml | Yaml
            | Json | Shell | Unknown => {}
        };
        let _ = _exhaust;
        for lang in langs {
            if !lang.has_grammar() {
                continue;
            }
            let ts_lang = ts_language_for(lang).unwrap_or_else(|| panic!("{lang} grammar missing"));
            let mut parser = tree_sitter_lib::Parser::new();
            parser.set_language(&ts_lang).unwrap_or_else(|e| {
                panic!("{lang} rejected by tree-sitter 0.27: {e}");
            });
        }
    }

    #[test]
    fn display_formatting() {
        assert_eq!(Language::Rust.to_string(), "Rust");
        assert_eq!(Language::CSharp.to_string(), "C#");
        assert_eq!(Language::Cpp.to_string(), "C++");
    }
}
