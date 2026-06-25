//! Dependency/import analysis using tree-sitter.

use std::path::Path;

use serde::Serialize;

use super::{Language, parse_source};

/// A single import/dependency extracted from a file.
#[derive(Debug, Clone, Serialize)]
pub struct Import {
    /// The import path or module name.
    pub path: String,
    /// 1-based line number.
    pub line: usize,
    /// The raw import statement text.
    pub raw: String,
}

/// Extract imports from source code.
pub fn extract_imports(source: &str, lang: Language) -> Vec<Import> {
    let Some((tree, _)) = parse_source(source, lang) else {
        return Vec::new();
    };

    let mut imports = Vec::new();
    collect_imports(tree.root_node(), source, lang, &mut imports);
    imports
}

/// Extract imports from a file.
pub fn extract_imports_from_file(path: &Path, lang_hint: Option<Language>) -> Vec<Import> {
    let lang = lang_hint.unwrap_or_else(|| Language::from_path(path));
    if !lang.has_grammar() {
        return Vec::new();
    }
    let Ok(source) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    extract_imports(&source, lang)
}

fn collect_imports(
    node: tree_sitter_lib::Node,
    source: &str,
    lang: Language,
    imports: &mut Vec<Import>,
) {
    match lang {
        Language::Rust => collect_rust_imports(node, source, imports),
        Language::Python => collect_python_imports(node, source, imports),
        Language::JavaScript | Language::TypeScript => collect_js_imports(node, source, imports),
        Language::Go => collect_go_imports(node, source, imports),
        Language::Java => collect_java_imports(node, source, imports),
        Language::C | Language::Cpp => collect_c_imports(node, source, imports),
        Language::Ruby => collect_ruby_imports(node, source, imports),
        Language::Php => collect_php_imports(node, source, imports),
        _ => collect_generic_imports(node, source, imports),
    }
}

// ---------------------------------------------------------------------------
// Language-specific import extractors
// ---------------------------------------------------------------------------

fn collect_rust_imports(node: tree_sitter_lib::Node, source: &str, imports: &mut Vec<Import>) {
    if node.kind() == "use_declaration" || node.kind() == "extern_crate_declaration" {
        if let Ok(text) = node.utf8_text(source.as_bytes()) {
            let path = extract_path_from_text(text, &["use ", "extern crate "], &[';']);
            imports.push(Import {
                path,
                line: node.start_position().row + 1,
                raw: text.trim().to_string(),
            });
        }
        return;
    }
    // mod declarations with a body are not imports; only `mod foo;` is
    if node.kind() == "mod_item" {
        if let Ok(text) = node.utf8_text(source.as_bytes())
            && text.trim().ends_with(';')
        {
            let path = extract_path_from_text(text, &["mod "], &[';']);
            imports.push(Import {
                path,
                line: node.start_position().row + 1,
                raw: text.trim().to_string(),
            });
        }
        return;
    }

    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            collect_rust_imports(cursor.node(), source, imports);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

fn collect_python_imports(node: tree_sitter_lib::Node, source: &str, imports: &mut Vec<Import>) {
    if node.kind() == "import_statement" || node.kind() == "import_from_statement" {
        if let Ok(text) = node.utf8_text(source.as_bytes()) {
            let path = extract_path_from_text(text, &["from ", "import "], &[]);
            imports.push(Import {
                path,
                line: node.start_position().row + 1,
                raw: text.trim().to_string(),
            });
        }
        return;
    }

    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            collect_python_imports(cursor.node(), source, imports);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

fn collect_js_imports(node: tree_sitter_lib::Node, source: &str, imports: &mut Vec<Import>) {
    if node.kind() == "import_statement" {
        if let Ok(text) = node.utf8_text(source.as_bytes()) {
            let path = extract_quoted_string(text).unwrap_or_else(|| text.trim().to_string());
            imports.push(Import {
                path,
                line: node.start_position().row + 1,
                raw: text.trim().to_string(),
            });
        }
        return;
    }

    // Also check for require() calls
    if node.kind() == "call_expression" {
        if let Ok(text) = node.utf8_text(source.as_bytes())
            && text.starts_with("require(")
        {
            let path = extract_quoted_string(text).unwrap_or_else(|| text.trim().to_string());
            imports.push(Import {
                path,
                line: node.start_position().row + 1,
                raw: text.trim().to_string(),
            });
        }
        return;
    }

    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            collect_js_imports(cursor.node(), source, imports);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

fn collect_go_imports(node: tree_sitter_lib::Node, source: &str, imports: &mut Vec<Import>) {
    if node.kind() == "import_declaration" {
        // Go imports can have a single spec or multiple in parens
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if child.kind() == "import_spec" || child.kind() == "import_spec_list" {
                    collect_go_import_specs(child, source, imports);
                } else if child.kind() == "interpreted_string_literal"
                    && let Ok(text) = child.utf8_text(source.as_bytes())
                {
                    let path = text.trim_matches('"').to_string();
                    imports.push(Import {
                        path,
                        line: child.start_position().row + 1,
                        raw: text.to_string(),
                    });
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
        return;
    }

    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            collect_go_imports(cursor.node(), source, imports);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

fn collect_go_import_specs(node: tree_sitter_lib::Node, source: &str, imports: &mut Vec<Import>) {
    if node.kind() == "import_spec" {
        // Find the string literal child
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "interpreted_string_literal"
                && let Ok(text) = child.utf8_text(source.as_bytes())
            {
                let path = text.trim_matches('"').to_string();
                imports.push(Import {
                    path,
                    line: child.start_position().row + 1,
                    raw: node
                        .utf8_text(source.as_bytes())
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                });
            }
        }
    } else {
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                collect_go_import_specs(cursor.node(), source, imports);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
}

fn collect_java_imports(node: tree_sitter_lib::Node, source: &str, imports: &mut Vec<Import>) {
    if node.kind() == "import_declaration" {
        if let Ok(text) = node.utf8_text(source.as_bytes()) {
            let path = extract_path_from_text(text, &["import ", "import static "], &[';']);
            imports.push(Import {
                path,
                line: node.start_position().row + 1,
                raw: text.trim().to_string(),
            });
        }
        return;
    }

    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            collect_java_imports(cursor.node(), source, imports);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

fn collect_c_imports(node: tree_sitter_lib::Node, source: &str, imports: &mut Vec<Import>) {
    if node.kind() == "preproc_include" {
        if let Ok(text) = node.utf8_text(source.as_bytes()) {
            let path = if let Some(p) = extract_angle_bracket_path(text) {
                p
            } else {
                extract_quoted_string(text).unwrap_or_else(|| text.trim().to_string())
            };
            imports.push(Import {
                path,
                line: node.start_position().row + 1,
                raw: text.trim().to_string(),
            });
        }
        return;
    }

    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            collect_c_imports(cursor.node(), source, imports);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

fn collect_ruby_imports(node: tree_sitter_lib::Node, source: &str, imports: &mut Vec<Import>) {
    if node.kind() == "call"
        && let Ok(text) = node.utf8_text(source.as_bytes())
        && (text.starts_with("require ") || text.starts_with("require_relative "))
    {
        let path = extract_quoted_string(text).unwrap_or_else(|| text.trim().to_string());
        imports.push(Import {
            path,
            line: node.start_position().row + 1,
            raw: text.trim().to_string(),
        });
    }

    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            collect_ruby_imports(cursor.node(), source, imports);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

fn collect_php_imports(node: tree_sitter_lib::Node, source: &str, imports: &mut Vec<Import>) {
    if node.kind() == "namespace_use_declaration" {
        if let Ok(text) = node.utf8_text(source.as_bytes()) {
            let path = extract_path_from_text(text, &["use "], &[';']);
            imports.push(Import {
                path,
                line: node.start_position().row + 1,
                raw: text.trim().to_string(),
            });
        }
        return;
    }

    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            collect_php_imports(cursor.node(), source, imports);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

fn collect_generic_imports(node: tree_sitter_lib::Node, source: &str, imports: &mut Vec<Import>) {
    let kind = node.kind();
    if kind.contains("import") || kind.contains("include") || kind == "use_declaration" {
        if let Ok(text) = node.utf8_text(source.as_bytes()) {
            imports.push(Import {
                path: text.trim().to_string(),
                line: node.start_position().row + 1,
                raw: text.trim().to_string(),
            });
        }
        return;
    }

    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            collect_generic_imports(cursor.node(), source, imports);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn extract_path_from_text(text: &str, prefixes: &[&str], suffixes: &[char]) -> String {
    let mut s = text.trim();
    for prefix in prefixes {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest;
            break;
        }
    }
    let mut result = s.trim().to_string();
    for suffix in suffixes {
        if result.ends_with(*suffix) {
            result.pop();
        }
    }
    result.trim().to_string()
}

fn extract_quoted_string(text: &str) -> Option<String> {
    // Find content between quotes (single or double)
    for quote in ['"', '\''] {
        if let Some(start) = text.find(quote)
            && let Some(end) = text[start + 1..].find(quote)
        {
            return Some(text[start + 1..start + 1 + end].to_string());
        }
    }
    None
}

fn extract_angle_bracket_path(text: &str) -> Option<String> {
    let start = text.find('<')?;
    let end = text.find('>')?;
    if start < end {
        Some(text[start + 1..end].to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_imports() {
        let source = r#"
use std::path::Path;
use crate::ast::Language;
extern crate serde;
mod helpers;
"#;
        let imports = extract_imports(source, Language::Rust);
        assert!(imports.len() >= 3);
        let paths: Vec<&str> = imports.iter().map(|i| i.path.as_str()).collect();
        assert!(paths.iter().any(|p| p.contains("std::path::Path")));
        assert!(paths.iter().any(|p| p.contains("crate::ast::Language")));
    }

    #[test]
    fn python_imports() {
        let source = r#"
import os
from pathlib import Path
import sys
"#;
        let imports = extract_imports(source, Language::Python);
        assert!(imports.len() >= 3);
    }

    #[test]
    fn js_imports() {
        let source = r#"
import React from 'react';
import { useState } from 'react';
const fs = require('fs');
"#;
        let imports = extract_imports(source, Language::JavaScript);
        assert!(imports.len() >= 2); // import statements
    }

    #[test]
    fn go_imports() {
        let source = r#"
package main

import (
    "fmt"
    "os"
)
"#;
        let imports = extract_imports(source, Language::Go);
        assert!(imports.len() >= 2);
        let paths: Vec<&str> = imports.iter().map(|i| i.path.as_str()).collect();
        assert!(paths.contains(&"fmt"));
        assert!(paths.contains(&"os"));
    }

    #[test]
    fn java_imports() {
        let source = r#"
import java.util.List;
import java.io.File;
"#;
        let imports = extract_imports(source, Language::Java);
        assert_eq!(imports.len(), 2);
    }

    #[test]
    fn c_includes() {
        let source = "#include <stdio.h>\n#include \"myheader.h\"\n";
        let imports = extract_imports(source, Language::C);
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].path, "stdio.h");
        assert_eq!(imports[1].path, "myheader.h");
    }

    #[test]
    fn typescript_imports() {
        let source = r#"
import { useState, useEffect } from 'react';
import axios from 'axios';
import type { Config } from './config';

const fs = require('fs');
const path = require('path');

function main() {
    const [state, setState] = useState(0);
    axios.get('/api/data');
}
"#;
        let imports = extract_imports(source, Language::TypeScript);
        assert!(
            imports.len() >= 3,
            "should find at least 3 imports, got {}",
            imports.len()
        );
        let paths: Vec<&str> = imports.iter().map(|i| i.path.as_str()).collect();
        assert!(paths.contains(&"react"), "should find react import");
        assert!(paths.contains(&"axios"), "should find axios import");
        // require() calls
        assert!(paths.contains(&"fs"), "should find fs require");
    }

    #[test]
    fn typescript_import_lines() {
        let source = r#"
import { Router } from 'express';
import helmet from 'helmet';
"#;
        let imports = extract_imports(source, Language::TypeScript);
        assert_eq!(imports.len(), 2);
        // Verify line numbers are correct (1-based)
        assert_eq!(imports[0].line, 2);
        assert_eq!(imports[1].line, 3);
    }

    #[test]
    fn java_imports_regular_and_static() {
        let source = r#"
package com.example.app;

import java.util.List;
import java.util.Map;
import java.io.IOException;
import static org.junit.Assert.*;

public class App {
    public void run() {
        List<String> items = List.of("a", "b");
    }
}
"#;
        let imports = extract_imports(source, Language::Java);
        assert!(
            imports.len() >= 4,
            "should find at least 4 imports, got {}",
            imports.len()
        );
        let paths: Vec<&str> = imports.iter().map(|i| i.path.as_str()).collect();
        assert!(
            paths.iter().any(|p| p.contains("java.util.List")),
            "should find java.util.List import"
        );
        assert!(
            paths.iter().any(|p| p.contains("java.util.Map")),
            "should find java.util.Map import"
        );
        assert!(
            paths.iter().any(|p| p.contains("java.io.IOException")),
            "should find java.io.IOException import"
        );
        assert!(
            paths.iter().any(|p| p.contains("org.junit.Assert")),
            "should find static org.junit.Assert import"
        );
    }

    #[test]
    fn java_import_raw_text() {
        let source = r#"
import java.util.ArrayList;
import static java.lang.Math.PI;
"#;
        let imports = extract_imports(source, Language::Java);
        assert_eq!(imports.len(), 2);
        assert!(imports[0].raw.contains("import java.util.ArrayList"));
        assert!(imports[1].raw.contains("import static java.lang.Math.PI"));
    }

    #[test]
    fn c_includes_system_and_local() {
        let source = r#"
#include <stdlib.h>
#include <string.h>
#include "config.h"
#include "utils/helpers.h"

void init() {
    char* buf = malloc(256);
    memset(buf, 0, 256);
}
"#;
        let imports = extract_imports(source, Language::C);
        assert_eq!(imports.len(), 4);
        let paths: Vec<&str> = imports.iter().map(|i| i.path.as_str()).collect();
        assert!(paths.contains(&"stdlib.h"));
        assert!(paths.contains(&"string.h"));
        assert!(paths.contains(&"config.h"));
        assert!(paths.contains(&"utils/helpers.h"));
    }

    #[test]
    fn cpp_includes() {
        let source = r#"
#include <iostream>
#include <vector>
#include <string>
#include "engine.hpp"

namespace app {
    class Runner {
    public:
        void run() {
            std::vector<std::string> items;
            std::cout << items.size() << std::endl;
        }
    };
}
"#;
        let imports = extract_imports(source, Language::Cpp);
        assert_eq!(imports.len(), 4);
        let paths: Vec<&str> = imports.iter().map(|i| i.path.as_str()).collect();
        assert!(paths.contains(&"iostream"));
        assert!(paths.contains(&"vector"));
        assert!(paths.contains(&"string"));
        assert!(paths.contains(&"engine.hpp"));
    }

    #[test]
    fn unknown_language_returns_empty() {
        let imports = extract_imports("anything", Language::Unknown);
        assert!(imports.is_empty());
    }

    #[test]
    fn extract_quoted_string_works() {
        assert_eq!(
            extract_quoted_string("require('fs')"),
            Some("fs".to_string())
        );
        assert_eq!(
            extract_quoted_string("import \"react\""),
            Some("react".to_string())
        );
        assert_eq!(extract_quoted_string("no quotes"), None);
    }
}
