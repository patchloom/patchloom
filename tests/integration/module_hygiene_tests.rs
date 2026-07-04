//! Module layout and size-policy hygiene for the structural rewrite program
//! (#1372 / #1376). Remaining domain monofiles are tracked in #1408.

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Production sources (excluding co-located `tests.rs` / `*_tests.rs`) over
/// 1000 lines must carry an explicit `size-waiver:` linked to an open tracker
/// issue (currently #1408; historically #1376).
#[test]
fn large_production_src_files_have_size_waiver() {
    let src_root = repo_root().join("src");
    let mut offenders = Vec::new();
    // Require a GitHub issue reference so waivers stay tracked (not free-form).
    let issue_ref = regex::Regex::new(r"size-waiver:.*#\d+").unwrap();

    for entry in walkdir_rs_files(&src_root) {
        let path = entry;
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name == "tests.rs" || name.ends_with("_tests.rs") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let lines = text.lines().count();
        if lines <= 1000 {
            continue;
        }
        if !issue_ref.is_match(&text) {
            offenders.push(format!(
                "{} ({} lines) missing `size-waiver: ... #NNNN`",
                path.strip_prefix(repo_root()).unwrap().display(),
                lines
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "large production files need `size-waiver: ... #NNNN` (see #1408):\n{}",
        offenders.join("\n")
    );
}

/// Plan and symbols modules stay under the 800-line soft budget for non-test
/// source (acceptance criterion in #1376).
#[test]
fn plan_and_symbols_core_modules_under_800_lines() {
    let checks = [
        "src/plan/mod.rs",
        "src/plan/operation.rs",
        "src/ast/symbols/mod.rs",
        "src/ast/extract_to_file.rs",
        "src/ast/symbol_extract.rs",
    ];
    for rel in checks {
        let path = repo_root().join(rel);
        assert!(path.exists(), "expected module path {rel}");
        let lines = fs::read_to_string(&path).unwrap().lines().count();
        assert!(
            lines <= 800,
            "{rel} is {lines} lines (soft budget 800 without size-waiver)"
        );
    }
}

/// Naming: extract-to-file and tree-sitter visitors must be distinct modules.
#[test]
fn extract_to_file_and_symbol_extract_are_distinct_modules() {
    let root = repo_root().join("src/ast");
    assert!(
        root.join("extract_to_file.rs").exists(),
        "extract_to_file.rs must exist for extract-a-symbol-to-a-file"
    );
    assert!(
        root.join("symbol_extract.rs").exists(),
        "symbol_extract.rs must exist for tree-sitter visitors"
    );
    // Old path may remain only as a deprecated shim in mod.rs, not as extract.rs.
    assert!(
        !root.join("extract.rs").exists(),
        "src/ast/extract.rs should be renamed to extract_to_file.rs"
    );

    let mod_rs = fs::read_to_string(root.join("mod.rs")).unwrap();
    assert!(
        mod_rs.contains("pub mod extract_to_file"),
        "ast/mod.rs must declare extract_to_file"
    );
    // #1386: deprecated shim removed (breaking / major).
    assert!(
        !mod_rs.contains("pub mod extract {"),
        "ast::extract shim must be removed; use extract_to_file only"
    );

    let extract_docs = fs::read_to_string(root.join("extract_to_file.rs")).unwrap();
    let visitor_docs = fs::read_to_string(root.join("symbol_extract.rs")).unwrap();
    assert!(
        extract_docs.contains("separate source file")
            || extract_docs.contains("separate file")
            || extract_docs.contains("extract_to_file"),
        "extract_to_file module docs must describe file extraction"
    );
    assert!(
        visitor_docs.contains("tree-sitter") || visitor_docs.contains("SymbolDef"),
        "symbol_extract docs must describe tree-sitter visitors"
    );
    assert!(
        visitor_docs.contains("extract_to_file") || visitor_docs.contains("not"),
        "symbol_extract docs must disambiguate from extract_to_file"
    );
}

/// Shared group position parser is the only `after:` implementation for groups.
#[test]
fn group_after_prefix_parsed_only_via_parse_group_position() {
    let group = fs::read_to_string(repo_root().join("src/ast/group.rs")).unwrap();
    assert!(
        group.contains("pub fn parse_group_position"),
        "shared parser must exist"
    );
    let ast_op = fs::read_to_string(repo_root().join("src/tx/ast_op.rs")).unwrap();
    assert!(
        ast_op.contains("parse_group_position"),
        "tx ast_op must use shared parse_group_position"
    );
    // No inline after: strip in ast_op for group path.
    let without_move = ast_op
        .lines()
        .filter(|l| !l.contains("parse_position") && !l.contains("//"))
        .collect::<Vec<_>>()
        .join("\n");
    // Allow after: only inside string literals of unrelated tests, not strip_prefix in ast_op.
    assert!(
        !without_move.contains("strip_prefix(\"after:\")"),
        "ast_op must not open-code after: stripping; use shared parsers"
    );
}

/// Historical rewrite re-exports must be marked deprecated with #1376.
#[test]
fn symbols_module_does_not_reexport_rewrite_helpers() {
    let symbols = fs::read_to_string(repo_root().join("src/ast/symbols/mod.rs")).unwrap();
    assert!(
        !symbols.contains("pub use crate::ast::rewrite::"),
        "symbols must not re-export rewrite helpers; use ast::rewrite"
    );
    assert!(
        !symbols.contains("FunctionSigEdit"),
        "FunctionSigEdit must not appear in symbols module after #1386"
    );
    let rewrite = fs::read_to_string(repo_root().join("src/ast/rewrite.rs")).unwrap();
    assert!(
        rewrite.contains("FunctionSigEdit"),
        "rewrite module must own FunctionSigEdit"
    );
}

fn walkdir_rs_files(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
    walk(root, &mut out);
    out
}
