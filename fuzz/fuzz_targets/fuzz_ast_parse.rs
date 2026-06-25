#![no_main]

use libfuzzer_sys::fuzz_target;
use patchloom::ast::{Language, parse_source, symbols::extract_symbols};

fuzz_target!(|data: &str| {
    // Tree-sitter parsing and symbol extraction must never panic,
    // regardless of source content or language.
    let langs = [
        Language::Rust,
        Language::Python,
        Language::Go,
        Language::TypeScript,
        Language::JavaScript,
        Language::Java,
        Language::C,
        Language::Cpp,
        Language::Ruby,
        Language::Php,
        Language::Shell,
    ];
    for lang in langs {
        let _ = parse_source(data, lang);
        let _ = extract_symbols(data, lang);
    }
});