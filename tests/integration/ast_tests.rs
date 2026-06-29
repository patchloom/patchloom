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
            "ast", "replace", "test.rs", "greet", "--from", "hello", "--to", "world", "--apply",
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
    patchloom_in(dir.path())
        .args(["ast", "search", "(function_item) @fn", "s.rs", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("fn f()"))
        .stdout(predicates::str::contains("let y = 42"));
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
    // Initialize minimal git repo so --from HEAD works for dispatch coverage.
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
    let v: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(v["valid"], serde_json::json!(false));
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
