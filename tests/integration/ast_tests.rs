use super::*;

#[test]
#[cfg(feature = "ast")]
fn test_ast_replace_apply_dispatch() {
    // Verifies the dispatch bug fix: `ast replace --apply` should actually
    // apply changes (previously WriteFlags were not merged for Replace).
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.rs");
    fs::write(&file, "fn greet() {\n    println!(\"hello\");\n}\n").unwrap();

    patchloom_in(dir.path())
        .args([
            "ast", "replace", "test.rs", "greet", "--old", "hello", "--new", "world", "--apply",
        ])
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("world"),
        "ast replace --apply should modify file: {content}"
    );
    assert!(
        !content.contains("hello"),
        "old text should be replaced: {content}"
    );
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_list_basic() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("lib.rs");
    fs::write(&f, "pub fn foo() {}\nstruct Bar;\n").unwrap();
    patchloom_in(dir.path())
        .args(["ast", "list", "lib.rs", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("foo"))
        .stdout(predicates::str::contains("Bar"));
}

/// `--json` for multi-symbol list must be one parseable JSON array, not
/// pretty multi-documents (agents use `json.loads` on full stdout).
#[test]
#[cfg(feature = "ast")]
fn test_ast_list_json_is_single_array() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("lib.rs");
    fs::write(&f, "pub fn foo() {}\nstruct Bar;\nfn baz() {}\n").unwrap();
    let out = patchloom_in(dir.path())
        .args(["ast", "list", "lib.rs", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    let val: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("ast list --json must be single JSON document: {e}\n{text}"));
    let arr = val
        .as_array()
        .unwrap_or_else(|| panic!("ast list --json must be a JSON array, got: {val}"));
    assert!(
        arr.len() >= 3,
        "expected at least 3 symbols, got {}",
        arr.len()
    );
    let names: Vec<&str> = arr
        .iter()
        .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
        .collect();
    assert!(names.contains(&"foo"), "missing foo in {names:?}");
    assert!(names.contains(&"Bar"), "missing Bar in {names:?}");
    assert!(names.contains(&"baz"), "missing baz in {names:?}");
}

/// Directory `ast list --json` must still be one array (not one array per file).
#[test]
#[cfg(feature = "ast")]
fn test_ast_list_dir_json_is_single_array() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    fs::write(dir.path().join("b.rs"), "fn b() {}\nfn c() {}\n").unwrap();
    let out = patchloom_in(dir.path())
        .args(["ast", "list", ".", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    let val: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("dir list --json must be single JSON: {e}\n{text}"));
    let arr = val.as_array().expect("dir list --json must be array");
    assert!(
        arr.len() >= 3,
        "expected symbols from both files, got {}",
        arr.len()
    );
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_read_basic() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("main.rs");
    fs::write(&f, "fn target() { let x=1; }\n").unwrap();
    patchloom_in(dir.path())
        .args(["ast", "read", "main.rs", "target"])
        .assert()
        .success()
        .stdout(predicates::str::contains("target"));
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_read_symbol_not_found_exits_3() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("main.rs");
    fs::write(&f, "fn target() { let x=1; }\n").unwrap();
    patchloom_in(dir.path())
        .args(["ast", "read", "main.rs", "nonexistent"])
        .assert()
        .code(3)
        .stderr(predicates::str::contains("not found"));
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_read_symbol_not_found_json() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("main.rs");
    fs::write(&f, "fn target() { let x=1; }\n").unwrap();
    patchloom_in(dir.path())
        .args(["ast", "read", "main.rs", "nonexistent", "--json"])
        .assert()
        .code(3)
        .stdout(
            predicates::str::contains(r#""ok":false"#)
                .or(predicates::str::contains(r#""ok": false"#)),
        );
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_validate_ok() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("ok.rs");
    fs::write(&f, "fn ok() {}\n").unwrap();
    patchloom_in(dir.path())
        .args(["ast", "validate", "ok.rs"])
        .assert()
        .success()
        .stdout(predicates::str::is_empty());
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_search_basic() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("s.rs");
    fs::write(&f, "fn f() { let y = 42; }\n").unwrap();
    // simple structural query for function item
    let out = patchloom_in(dir.path())
        .args(["ast", "search", "(function_item) @fn", "s.rs", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("ast search --json must be single JSON: {e}\n{text}"));
    let arr = v.as_array().expect("ast search --json must be array");
    assert!(!arr.is_empty());
    let joined = text;
    assert!(joined.contains("fn f()"), "{joined}");
    assert!(joined.contains("let y = 42"), "{joined}");
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_refs_basic() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("r.rs");
    fs::write(&f, "fn callee() {}\nfn caller() { callee(); }\n").unwrap();
    patchloom_in(dir.path())
        .args(["ast", "refs", "callee", "r.rs", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("caller"));
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_deps_basic() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("d.rs");
    fs::write(&f, "use std::collections::HashMap;\n").unwrap();
    patchloom_in(dir.path())
        .args(["ast", "deps", "d.rs", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("HashMap"))
        .stdout(predicates::str::contains("std"));
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_map_basic() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("m.rs");
    fs::write(&f, "fn a(){} fn b(){ a(); }\n").unwrap();
    patchloom_in(dir.path())
        .args(["ast", "map", ".", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"name\": \"a\""))
        .stdout(predicates::str::contains("\"name\": \"b\""));
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_impact_basic() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("i.rs");
    fs::write(&f, "fn entry(){ helper(); }\nfn helper(){}\n").unwrap();
    patchloom_in(dir.path())
        .args(["ast", "impact", "helper", "i.rs", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("entry"));
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_diff_basic() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("diff.rs");
    fs::write(&f, "fn v1(){}\n").unwrap();
    // Initialize minimal git repo so --old HEAD works for dispatch coverage.
    // Use explicit author to make commit succeed in CI without global git config.
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .env("GIT_AUTHOR_NAME", "CI")
            .env("GIT_AUTHOR_EMAIL", "ci@example.com")
            .env("GIT_COMMITTER_NAME", "CI")
            .env("GIT_COMMITTER_EMAIL", "ci@example.com")
            .status()
            .expect("git command failed to spawn")
    };
    assert!(git(&["init", "-q"]).success(), "git init failed");
    assert!(git(&["add", "-A"]).success(), "git add failed");
    assert!(
        git(&["commit", "-q", "-m", "init"]).success(),
        "git commit failed; no HEAD"
    );
    // Modify after commit so there is a structural diff vs HEAD.
    fs::write(&f, "fn v1(){}\nfn v2(){}\n").unwrap();
    patchloom_in(dir.path())
        .args(["ast", "diff", "diff.rs", "--from", "HEAD", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"name\": \"v2\""))
        .stdout(predicates::str::contains("\"change\": \"added\""));
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_list_nonexistent_fails() {
    let dir = TempDir::new().unwrap();
    patchloom_in(dir.path())
        .args(["ast", "list", "no_such.rs"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("path not found"));
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_validate_invalid_syntax_reports_failure() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("bad.rs");
    fs::write(&f, "fn broken( { \n").unwrap();
    patchloom_in(dir.path())
        .args(["ast", "validate", "bad.rs"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("INVALID (Rust)"));
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_list_jsonl_outputs_compact_lines() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("lib.rs");
    fs::write(&f, "pub fn foo() {}\nstruct Bar;\n").unwrap();
    let out = patchloom_in(dir.path())
        .args(["ast", "list", "lib.rs", "--jsonl"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines.len() >= 2,
        "expected at least 2 JSONL lines for 2 symbols"
    );
    for line in &lines {
        let v: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("line is not valid JSON: {e}: {line}"));
        assert!(v.is_object(), "each JSONL line should be an object");
        // Compact: no pretty-printing (no leading whitespace on first char).
        assert!(
            !line.starts_with(' '),
            "JSONL should be compact, not pretty"
        );
    }
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_validate_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("ok.rs");
    fs::write(&f, "fn main() {}\n").unwrap();
    let out = patchloom_in(dir.path())
        .args(["ast", "validate", "ok.rs", "--jsonl"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    let v: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(v["valid"], serde_json::json!(true));
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_validate_json_exit_code_on_invalid_file() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("bad.rs");
    fs::write(&f, "fn broken( {}\n").unwrap();
    let out = patchloom_in(dir.path())
        .args(["ast", "validate", "bad.rs", "--json"])
        .assert()
        .code(1) // exit::FAILURE, not SUCCESS
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    // Multi-result commands emit a JSON array (even for a single file).
    let v: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    let arr = v.as_array().expect("validate --json must be array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["valid"], serde_json::json!(false));
}

/// Multi-file validate --json must be one array, not multi-document JSON.
#[test]
#[cfg(feature = "ast")]
fn test_ast_validate_multi_file_json_is_single_array() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("ok.rs"), "fn ok() {}\n").unwrap();
    fs::write(dir.path().join("also.rs"), "fn also() {}\n").unwrap();
    let out = patchloom_in(dir.path())
        .args(["ast", "validate", ".", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    let v: serde_json::Value = serde_json::from_str(text.trim())
        .unwrap_or_else(|e| panic!("validate multi --json must be single JSON: {e}\n{text}"));
    let arr = v.as_array().expect("validate --json must be array");
    assert!(arr.len() >= 2, "expected both files, got {}", arr.len());
    assert!(arr.iter().all(|e| e["valid"] == true));
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_search_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("lib.rs");
    fs::write(&f, "fn alpha() {}\nfn beta() {}\n").unwrap();
    let out = patchloom_in(dir.path())
        .args(["ast", "search", "(function_item) @fn", "lib.rs", "--jsonl"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines.len() >= 2,
        "should have at least 2 JSONL lines for 2 matches"
    );
    for line in &lines {
        let _: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("not valid JSON: {e}: {line}"));
    }
}

// Regression: --max-results limited per-file but not globally, so N files
// with matches could produce up to N * max_results total.
#[test]
#[cfg(feature = "ast")]
fn test_ast_search_max_results_limits_globally() {
    let dir = TempDir::new().unwrap();
    // Create two files, each with 3 function_item matches.
    fs::write(
        dir.path().join("a.rs"),
        "fn a1() {}\nfn a2() {}\nfn a3() {}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("b.rs"),
        "fn b1() {}\nfn b2() {}\nfn b3() {}\n",
    )
    .unwrap();
    let out = patchloom_in(dir.path())
        .args([
            "ast",
            "search",
            "(function_item) @fn",
            ".",
            "--jsonl",
            "--max-results",
            "2",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    let count = text.lines().count();
    assert!(
        count <= 2,
        "max-results 2 should produce at most 2 results globally, got {count}"
    );
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_map_jsonl_per_entry_output() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("map.rs");
    fs::write(&f, "fn caller() { helper(); }\nfn helper() {}\n").unwrap();

    // JSONL mode: one compact JSON object per line.
    let jsonl_out = patchloom_in(dir.path())
        .args(["ast", "map", ".", "--jsonl"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let jsonl_text = String::from_utf8(jsonl_out).unwrap();
    let jsonl_lines: Vec<&str> = jsonl_text.lines().collect();
    assert!(
        jsonl_lines.len() >= 2,
        "expected at least 2 JSONL lines for 2 map entries, got {}",
        jsonl_lines.len()
    );
    for line in &jsonl_lines {
        let v: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("JSONL line is not valid JSON: {e}: {line}"));
        assert!(
            v.is_object(),
            "each JSONL line should be an object, not an array"
        );
        assert!(
            !line.starts_with(' '),
            "JSONL should be compact, not pretty-printed"
        );
    }

    // JSON mode: single array containing all entries.
    let json_out = patchloom_in(dir.path())
        .args(["ast", "map", ".", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json_text = String::from_utf8(json_out).unwrap();
    let json_val: serde_json::Value = serde_json::from_str(&json_text)
        .unwrap_or_else(|e| panic!("JSON output is not valid: {e}"));
    let json_arr = json_val.as_array().expect("--json should emit an array");

    // Line count in JSONL must equal array length in JSON.
    assert_eq!(
        jsonl_lines.len(),
        json_arr.len(),
        "JSONL line count ({}) should equal JSON array length ({})",
        jsonl_lines.len(),
        json_arr.len()
    );
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_list_unsupported_file_reports_language() {
    // Regression test for #937: unsupported language should report detected
    // language name and list supported languages.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.csv");
    fs::write(&file, "a,b,c\n1,2,3\n").unwrap();
    patchloom_in(dir.path())
        .args(["ast", "list", "data.csv"])
        .assert()
        .code(3) // NO_MATCHES
        .stderr(predicates::str::contains("Unsupported language"))
        .stderr(predicates::str::contains("Rust"));
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_list_unsupported_quiet_suppresses_stderr() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.csv");
    fs::write(&file, "a,b,c\n1,2,3\n").unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--quiet", "--cwd"])
        .arg(dir.path())
        .args(["ast", "list", "data.csv"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.is_empty(),
        "quiet should suppress stderr, got: {stderr}"
    );
}

// --- NO_MATCHES (exit 3) tests for AST subcommands ---
// These verify the exit code contract agents rely on for control flow.

#[test]
#[cfg(feature = "ast")]
fn test_ast_search_no_match_exits_3() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("lib.rs");
    fs::write(&f, "fn foo() {}\n").unwrap();
    // Query for class_declaration which does not exist in Rust
    patchloom_in(dir.path())
        .args(["ast", "search", "(class_declaration) @cls", "lib.rs"])
        .assert()
        .code(3);
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_refs_no_references_exits_3() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("r.rs");
    // `lonely` is defined but never called, so refs should find nothing.
    fs::write(&f, "fn lonely() {}\n").unwrap();
    patchloom_in(dir.path())
        .args(["ast", "refs", "lonely", "r.rs"])
        .assert()
        .code(3);
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_deps_no_imports_exits_3() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("d.rs");
    // No use/import statements
    fs::write(&f, "fn no_deps() {}\n").unwrap();
    patchloom_in(dir.path())
        .args(["ast", "deps", "d.rs"])
        .assert()
        .code(3);
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_map_no_symbols_exits_3() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("empty.rs");
    // A file with only a comment has no symbols
    fs::write(&f, "// just a comment\n").unwrap();
    patchloom_in(dir.path())
        .args(["ast", "map", "."])
        .assert()
        .code(3);
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_rename_apply_exits_0() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("rename.rs");
    fs::write(&f, "fn old_name() {}\nfn caller() { old_name(); }\n").unwrap();
    patchloom_in(dir.path())
        .args([
            "ast",
            "rename",
            "rename.rs",
            "--old",
            "old_name",
            "--new",
            "new_name",
            "--apply",
        ])
        .assert()
        .code(0);
    let content = fs::read_to_string(&f).unwrap();
    assert!(
        content.contains("new_name"),
        "ast rename --apply should rename symbol: {content}"
    );
    assert!(
        !content.contains("old_name"),
        "old symbol name should be replaced: {content}"
    );
}

/// Identity rename (old==new) must not claim files_changed or applied.
#[test]
#[cfg(feature = "ast")]
fn test_ast_rename_identity_json_files_changed_zero() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("same.rs");
    fs::write(&f, "fn foo() {}\n").unwrap();
    let out = patchloom_in(dir.path())
        .args([
            "--json", "ast", "rename", "same.rs", "--old", "foo", "--new", "foo", "--apply",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(
        v["files_changed"], 0,
        "identity rename must not claim a change: {v}"
    );
    assert_eq!(
        v["applied"], false,
        "identity rename must set applied:false: {v}"
    );
    assert_eq!(fs::read_to_string(&f).unwrap(), "fn foo() {}\n");
}

/// Legacy path-last positionals are no longer accepted (canonical: path + --old/--new).
#[test]
#[cfg(feature = "ast")]
fn test_ast_rename_legacy_positionals_rejected() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("rename.rs");
    fs::write(&f, "fn alpha() {}\n").unwrap();
    // Clap usage error → FAILURE (1), not CHANGES_DETECTED (2).
    patchloom_in(dir.path())
        .args(["ast", "rename", "alpha", "beta", "rename.rs", "--apply"])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("--old"))
        .stderr(predicates::str::contains("--new"));
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_rename_nonexistent_symbol_exits_3() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("rename.rs");
    fs::write(&f, "fn real() {}\n").unwrap();
    patchloom_in(dir.path())
        .args([
            "ast",
            "rename",
            "rename.rs",
            "--old",
            "nonexistent",
            "--new",
            "new_name",
            "--apply",
        ])
        .assert()
        .code(3);
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_rename_check_mode_exits_2() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("rename.rs");
    fs::write(&f, "fn old_name() {}\n").unwrap();
    patchloom_in(dir.path())
        .args([
            "ast",
            "rename",
            "rename.rs",
            "--old",
            "old_name",
            "--new",
            "new_name",
            "--check",
        ])
        .assert()
        .code(2);
    // File should be unchanged in check mode.
    let content = fs::read_to_string(&f).unwrap();
    assert!(
        content.contains("old_name"),
        "check mode should not modify file: {content}"
    );
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_replace_missing_symbol_exits_3() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("test.rs");
    fs::write(&f, "fn real() {}\n").unwrap();
    patchloom_in(dir.path())
        .args([
            "ast",
            "replace",
            "test.rs",
            "nonexistent",
            "--old",
            "x",
            "--new",
            "y",
            "--apply",
        ])
        .assert()
        .code(3);
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_impact_no_refs_exits_3() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("i.rs");
    // `isolated` is defined but never referenced
    fs::write(&f, "fn isolated() {}\n").unwrap();
    patchloom_in(dir.path())
        .args(["ast", "impact", "isolated", "i.rs"])
        .assert()
        .code(3);
}

#[test]
#[cfg(feature = "ast")]
fn test_ast_diff_no_changes_exits_3() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("same.rs");
    fs::write(&f, "fn stable() {}\n").unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .env("GIT_AUTHOR_NAME", "CI")
            .env("GIT_AUTHOR_EMAIL", "ci@example.com")
            .env("GIT_COMMITTER_NAME", "CI")
            .env("GIT_COMMITTER_EMAIL", "ci@example.com")
            .status()
            .expect("git command failed to spawn")
    };
    assert!(git(&["init", "-q"]).success());
    assert!(git(&["add", "-A"]).success());
    assert!(git(&["commit", "-q", "-m", "init"]).success());
    // File unchanged after commit, so diff should find no structural changes.
    patchloom_in(dir.path())
        .args(["ast", "diff", "same.rs", "--from", "HEAD"])
        .assert()
        .code(3);
}

/// `--glob` flag should filter files in AST directory scanning (#1171).
#[test]
fn test_ast_list_respects_glob_flag() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn foo() {}\n").unwrap();
    fs::write(dir.path().join("main.py"), "def bar(): pass\n").unwrap();

    // Without glob: both files should produce output.
    let out_all = patchloom_in(dir.path())
        .args(["ast", "list", "."])
        .output()
        .unwrap();
    let stdout_all = String::from_utf8_lossy(&out_all.stdout);
    assert!(stdout_all.contains("foo"), "should list foo from lib.rs");
    assert!(stdout_all.contains("bar"), "should list bar from main.py");

    // With glob *.rs: only Rust file should produce output.
    let out_rs = patchloom_in(dir.path())
        .args(["ast", "list", ".", "--glob", "*.rs"])
        .output()
        .unwrap();
    let stdout_rs = String::from_utf8_lossy(&out_rs.stdout);
    assert!(stdout_rs.contains("foo"), "should list foo from lib.rs");
    assert!(
        !stdout_rs.contains("bar"),
        "should NOT list bar from main.py"
    );
}

// ---------------------------------------------------------------------------
// --contain on ast list (MPI 2026-07-07 cycle 1 QA)
// ---------------------------------------------------------------------------

#[cfg(feature = "ast")]
#[test]
fn test_ast_list_contain_rejects_parent_escape() {
    let dir = TempDir::new().unwrap();
    let escape_name = format!(
        "patchloom-ast-escape-{}.rs",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let outside = dir.path().parent().unwrap().join(&escape_name);
    fs::write(&outside, "fn secret() {}\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "ast", "list", &format!("../{escape_name}")])
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );

    let _ = fs::remove_file(&outside);
}

#[cfg(feature = "ast")]
#[test]
fn test_ast_deps_contain_rejects_parent_escape() {
    let dir = TempDir::new().unwrap();
    let escape_name = format!(
        "patchloom-ast-deps-escape-{}.rs",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let outside = dir.path().parent().unwrap().join(&escape_name);
    fs::write(&outside, "fn secret() {}\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "ast", "deps", &format!("../{escape_name}")])
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );

    let _ = fs::remove_file(&outside);
}

#[cfg(feature = "ast")]
#[test]
fn test_ast_map_contain_rejects_parent_escape() {
    let dir = TempDir::new().unwrap();
    let escape_name = format!(
        "patchloom-ast-map-escape-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let outside = dir.path().parent().unwrap().join(&escape_name);
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("lib.rs"), "fn secret() {}\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "ast", "map", &format!("../{escape_name}")])
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );

    let _ = fs::remove_dir_all(&outside);
}

#[cfg(feature = "ast")]
#[test]
fn test_ast_list_contain_rejects_absolute_outside_workspace() {
    let dir = TempDir::new().unwrap();
    let outside = std::env::temp_dir().join(format!(
        "patchloom-ast-abs-escape-{}.rs",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    fs::write(&outside, "fn secret() {}\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "ast", "list"])
        .arg(&outside)
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );

    let _ = fs::remove_file(&outside);
}

#[cfg(feature = "ast")]
#[test]
fn test_ast_list_without_contain_allows_parent_escape() {
    let dir = TempDir::new().unwrap();
    let escape_name = format!(
        "patchloom-ast-open-escape-{}.rs",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let outside = dir.path().parent().unwrap().join(&escape_name);
    fs::write(&outside, "fn open_secret() {}\n").unwrap();

    // Default CLI is unrestricted: reading ../ is allowed without --contain
    // (same trust model as create without --contain).
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["ast", "list", &format!("../{escape_name}")])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("open_secret"));

    let _ = fs::remove_file(&outside);
}

#[cfg(feature = "ast")]
#[test]
fn test_ast_list_empty_path_rejected() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn keep() {}\n").unwrap();

    // Empty path previously joined to cwd and listed the entire workspace
    // (agent footgun when a path argument is omitted or blank).
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["ast", "list", ""])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("path must not be empty"));
}

#[cfg(feature = "ast")]
#[test]
fn test_ast_list_whitespace_only_path_rejected() {
    let dir = TempDir::new().unwrap();
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["ast", "list", "   "])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("path must not be empty"));
}

// ---------------------------------------------------------------------------
// ast validate must not vacuous-succeed on unsupported / empty targets
// ---------------------------------------------------------------------------

#[cfg(feature = "ast")]
#[test]
fn test_ast_validate_unsupported_language_is_no_matches() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("notes.txt"), "not source code\n").unwrap();

    let output = patchloom_in(dir.path())
        .args(["--json", "ast", "validate", "notes.txt"])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(3),
        "unsupported language must not exit 0; stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["error_kind"], "no_matches");
    assert!(
        json["error"]
            .as_str()
            .unwrap_or("")
            .contains("Unsupported language"),
        "error should name unsupported language: {json}"
    );
}

#[cfg(feature = "ast")]
#[test]
fn test_ast_validate_empty_dir_is_no_matches() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("empty")).unwrap();
    fs::write(dir.path().join("empty/readme.md"), "# no grammar\n").unwrap();

    let output = patchloom_in(dir.path())
        .args(["--json", "ast", "validate", "empty"])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(3),
        "dir with no source files must not vacuous-succeed; stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["error_kind"], "no_matches");
}

#[cfg(feature = "ast")]
#[test]
fn test_ast_validate_ok_rust_still_succeeds() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("ok.rs"), "fn main() {}\n").unwrap();

    let output = patchloom_in(dir.path())
        .args(["--json", "ast", "validate", "ok.rs"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let arr = json.as_array().expect("validate --json is an array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["valid"], true);
}

/// Sole explicit binary AST paths must be invalid_input, not no_matches /
/// unsupported-language / missing-symbol (NUL is valid UTF-8).
#[cfg(feature = "ast")]
#[test]
fn test_ast_sole_binary_is_invalid_input() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("blob.bin"), b"hello\x00world").unwrap();

    for args in [
        vec!["--json", "ast", "list", "blob.bin"],
        vec!["--json", "ast", "validate", "blob.bin"],
        vec![
            "--json", "ast", "rename", "blob.bin", "--old", "foo", "--new", "bar",
        ],
    ] {
        let output = patchloom_in(dir.path()).args(&args).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "args={args:?} stdout={}",
            String::from_utf8_lossy(&output.stdout)
        );
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            json["error_kind"], "invalid_input",
            "args={args:?} json={json}"
        );
        assert!(
            json["error"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .contains("binary"),
            "args={args:?} json={json}"
        );
    }
}

/// Directory rename walk: unreadable files must not be reported as symbol
/// not found when they may have masked the scan (SoftTextSkip::Unreadable).
#[cfg(all(unix, feature = "ast"))]
#[test]
fn test_ast_rename_dir_unreadable_not_no_matches() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir(&src).unwrap();
    // No matching symbol; only an unreadable file. Soft-skip must not look
    // like "symbol not found".
    let locked = src.join("locked.rs");
    fs::write(&locked, "fn secret() {}\n").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    // Root can still read mode-000; skip when permissions do not block.
    if fs::read_to_string(&locked).is_ok() {
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
        return;
    }

    let output = patchloom_in(dir.path())
        .args([
            "--json", "ast", "rename", "src", "--old", "secret", "--new", "public",
        ])
        .output()
        .unwrap();
    let _ = fs::set_permissions(&locked, fs::Permissions::from_mode(0o644));

    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["error_kind"], "invalid_input", "{json}");
    let err = json["error"].as_str().unwrap_or("");
    assert!(
        err.contains("could not read") && err.contains("not reporting as symbol"),
        "expected unreadable mask message, got: {json}"
    );
    // Real no_matches uses "symbol '…' not found"; must not use that shape.
    assert!(
        !err.contains("symbol '") && !err.contains("no_matches"),
        "must not use no_matches shape: {json}"
    );
}

/// Content SoftSkip (binary) in a dir with no matches is still no_matches.
#[cfg(feature = "ast")]
#[test]
fn test_ast_rename_dir_binary_only_is_no_matches() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("blob.bin"), b"fn secret()\x00{}\n").unwrap();

    let output = patchloom_in(dir.path())
        .args([
            "--json", "ast", "rename", "src", "--old", "secret", "--new", "public",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(3),
        "stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["error_kind"], "no_matches", "{json}");
}

/// Directory ast list with only unreadable sources is not no_matches.
#[test]
fn test_ast_list_dir_all_unreadable_not_no_matches() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.rs");
    fs::write(&a, "fn foo() {}\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&a, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read_to_string(&a).is_ok() {
            fs::set_permissions(&a, fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }
        let out = Command::cargo_bin("patchloom")
            .unwrap()
            .args(["--json", "--cwd"])
            .arg(dir.path())
            .args(["ast", "list", "."])
            .output()
            .unwrap();
        let _ = fs::set_permissions(&a, fs::Permissions::from_mode(0o644));
        assert_eq!(
            out.status.code(),
            Some(1),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["error_kind"], "invalid_input", "{v}");
        assert_ne!(v["error_kind"], "no_matches", "{v}");
    }
}
