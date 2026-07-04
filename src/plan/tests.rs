use super::*;

#[cfg(feature = "cli")]
#[test]
fn json_escape_handles_special_chars() {
    // Backslash
    assert_eq!(json_escape(r#"a\b"#), r#"a\\b"#);
    // Quotes
    assert_eq!(json_escape(r#"a"b"#), r#"a\"b"#);
    // Newlines
    assert_eq!(json_escape("a\nb"), r#"a\nb"#);
    // Combined
    assert_eq!(json_escape("he said \"hi\"\n"), r#"he said \"hi\"\n"#);
    // Plain string (no escaping needed)
    assert_eq!(json_escape("hello"), "hello");
}

#[cfg(feature = "cli")]
#[test]
fn substitute_single_pass_no_cross_contamination() {
    // If {path} expands to a value containing "{name}", the {name}
    // placeholder must NOT be substituted again.
    let template = r#"{"path": "{path}", "name": "{name}"}"#;
    let vars: &[(&str, String)] = &[
        ("{path}", "{name}/test.txt".into()),
        ("{name}", "test.txt".into()),
    ];
    let result = substitute_single_pass(template, vars);
    assert_eq!(result, r#"{"path": "{name}/test.txt", "name": "test.txt"}"#);
}

#[cfg(feature = "cli")]
#[test]
fn substitute_single_pass_basic() {
    let template = "file: {path}, dir: {dir}";
    let vars: &[(&str, String)] = &[("{path}", "src/main.rs".into()), ("{dir}", "src".into())];
    let result = substitute_single_pass(template, vars);
    assert_eq!(result, "file: src/main.rs, dir: src");
}

/// Regression: substitute_single_pass must handle multi-byte UTF-8
/// characters correctly (not corrupt them via byte-as-char casting).
#[cfg(feature = "cli")]
#[test]
fn substitute_single_pass_preserves_utf8() {
    let template = r#"{"path": "{path}", "to": "résumé café"}"#;
    let vars: &[(&str, String)] = &[("{path}", "src/main.rs".into())];
    let result = substitute_single_pass(template, vars);
    assert_eq!(
        result, r#"{"path": "src/main.rs", "to": "résumé café"}"#,
        "multi-byte UTF-8 characters must survive template expansion"
    );
}

#[test]
fn has_lifecycle_steps_none() {
    let plan = Plan {
        version: SCHEMA_VERSION,
        operations: Vec::new(),
        format: None,
        validate: None,
        verify: None,
        cwd: None,
        strict: None,
        write_policy: None,
        for_each: None,
    };
    assert!(!plan.has_lifecycle_steps());
}

#[test]
fn has_lifecycle_steps_empty_vecs() {
    let plan = Plan {
        version: SCHEMA_VERSION,
        operations: Vec::new(),
        format: Some(Vec::new()),
        validate: Some(Vec::new()),
        verify: None,
        cwd: None,
        strict: None,
        write_policy: None,
        for_each: None,
    };
    assert!(!plan.has_lifecycle_steps());
}

#[test]
fn has_lifecycle_steps_with_format() {
    let plan = Plan {
        version: SCHEMA_VERSION,
        operations: Vec::new(),
        format: Some(vec![FormatStep {
            cmd: "cargo fmt".into(),
            timeout: None,
        }]),
        validate: None,
        verify: None,
        cwd: None,
        strict: None,
        write_policy: None,
        for_each: None,
    };
    assert!(plan.has_lifecycle_steps());
}

#[test]
fn has_lifecycle_steps_with_validate() {
    let plan = Plan {
        version: SCHEMA_VERSION,
        operations: Vec::new(),
        format: None,
        validate: Some(vec![ValidationStep {
            cmd: "cargo clippy".into(),
            timeout: None,
            required: Some(true),
        }]),
        verify: None,
        cwd: None,
        strict: None,
        write_policy: None,
        for_each: None,
    };
    assert!(plan.has_lifecycle_steps());
}

#[test]
fn parse_minimal_plan() {
    let json = r#"{"version": 1, "operations": [{"op": "replace", "old": "a", "new": "b"}]}"#;
    let plan = parse_plan(json).unwrap();
    assert!(plan.cwd.is_none());
    assert!(plan.write_policy.is_none());
    assert!(plan.validate.is_none());
    assert_eq!(plan.version, 1);
    assert_eq!(plan.operations.len(), 1);
}

#[test]
fn parse_plan_version_field_accepted() {
    let json = r#"{"version": 1, "operations": [{"op": "replace", "old": "a", "new": "b"}]}"#;
    let plan = parse_plan(json).unwrap();
    assert_eq!(plan.version, 1);
}

#[test]
fn parse_plan_without_version_defaults_to_1() {
    let json = r#"{"operations": [{"op": "replace", "old": "a", "new": "b"}]}"#;
    let plan = parse_plan(json).unwrap();
    assert_eq!(plan.version, 1);
}

#[test]
fn parse_plan_with_all_fields() {
    let json = r#"{
            "version": 1,
            "cwd": "/tmp",
            "write_policy": {"ensure_final_newline": true, "normalize_eol": "lf"},
            "operations": [{"op": "file.create", "path": "f.txt", "content": "hi"}],
            "validate": [{"cmd": "echo ok"}]
        }"#;
    let plan = parse_plan(json).unwrap();
    assert_eq!(plan.cwd.as_deref(), Some("/tmp"));
    let wp = plan.write_policy.unwrap();
    assert_eq!(wp.ensure_final_newline, Some(true));
    assert_eq!(wp.normalize_eol.as_deref(), Some("lf"));
    assert!(plan.validate.unwrap()[0].required.is_none());
}

#[test]
fn parse_plan_unknown_op_fails() {
    let json = r#"{"version": 1, "operations": [{"op": "unknown", "x": 1}]}"#;
    assert!(parse_plan(json).is_err());
}

#[test]
fn parse_plan_missing_operations_fails() {
    let json = r#"{"version": 1, "cwd": "/tmp"}"#;
    assert!(parse_plan(json).is_err());
}

#[test]
#[cfg(feature = "ast")]
fn parse_all_operation_variants() {
    let json = r#"{"version": 1, "operations": [
            {"op": "replace", "old": "a", "new": "b"},
            {"op": "replace", "old": "a", "new": "b", "nth": 2},
            {"op": "doc.set", "path": "f.json", "selector": "k", "value": 1},
            {"op": "doc.delete", "path": "f.json", "selector": "k"},
            {"op": "doc.merge", "path": "f.json", "value": {}},
            {"op": "doc.append", "path": "f.json", "selector": "arr", "value": 1},
            {"op": "doc.prepend", "path": "f.json", "selector": "arr", "value": 0},
            {"op": "doc.update", "path": "f.json", "selector": "k", "value": 2},
            {"op": "doc.move", "path": "f.json", "from": "a", "to": "b"},
            {"op": "doc.ensure", "path": "f.json", "selector": "k", "value": 1},
            {"op": "doc.delete_where", "path": "f.json", "selector": "arr", "predicate": "name=x"},
            {"op": "md.replace_section", "path": "f.md", "heading": "H", "content": "c"},
            {"op": "md.insert_after_heading", "path": "f.md", "heading": "H", "content": "c"},
            {"op": "md.insert_before_heading", "path": "f.md", "heading": "H", "content": "c"},
            {"op": "md.upsert_bullet", "path": "f.md", "heading": "H", "bullet": "- item"},
            {"op": "md.table_append", "path": "f.md", "heading": "H", "row": "| a | b |"},
            {"op": "md.move_section", "path": "src.md", "heading": "FAQ", "before": "License"},
            {"op": "md.move_section", "path": "src.md", "heading": "Appendix", "to": "dest.md", "after": "Body"},
            {"op": "md.dedupe_headings", "path": "f.md"},
            {"op": "tidy.fix", "path": "f.txt"},
            {"op": "tidy.fix", "path": "f.txt", "trim_trailing_whitespace": true, "normalize_eol": "lf"},
            {"op": "file.append", "path": "f.txt", "content": "extra"},
            {"op": "file.create", "path": "f.txt", "content": "c"},
            {"op": "file.create", "path": "g.txt", "content": "c", "force": true},
            {"op": "file.delete", "path": "f.txt"},
            {"op": "file.rename", "from": "old.txt", "to": "new.txt"},
            {"op": "file.rename", "from": "a.txt", "to": "b.txt", "force": true},
            {"op": "patch.apply", "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -1 +1 @@\n-a\n+b"},
            {"op": "read", "path": "f.txt"},
            {"op": "read", "path": "f.txt", "lines": "1:10"},
            {"op": "search", "path": "f.txt", "pattern": "hello"},
            {"op": "search", "path": "f.txt", "pattern": "he.*o", "regex": true, "case_insensitive": true, "multiline": true},
            {"op": "search", "path": "f.txt", "pattern": "TODO", "invert_match": true, "assert_count": 5},
            {"op": "search", "path": ".", "pattern": "foo", "literal": true, "exclude_patterns": ["target/**"], "custom_ignore_filenames": [".blineignore"], "max_results": 10},
            {"op": "ast.rename", "path": "f.rs", "old": "Foo", "new": "Bar"},
            {"op": "ast.replace", "path": "f.rs", "symbol": "main", "old": "a", "new": "b"},
            {"op": "ast.insert", "path": "f.rs", "content": "fn new() {}", "after": "main"},
            {"op": "ast.wrap", "path": "f.rs", "symbols": ["helper"], "wrapper": "mod internal"},
            {"op": "ast.imports", "path": "f.rs", "add": ["use std::io;"]},
            {"op": "ast.reorder", "path": "f.rs", "order": "alphabetical"},
            {"op": "ast.reorder", "path": "f.rs", "order": ["b", "a"], "inside": "mod tests"},
            {"op": "ast.group", "path": "f.rs", "module": "tests", "symbols": ["test_a"]},
            {"op": "ast.move", "path": "src.rs", "target": "dst.rs", "symbols": ["foo"]},
            {"op": "ast.extract_to_file", "source": "lib.rs", "symbol": "tests", "target": "lib_tests.rs"},
            {"op": "ast.split", "source": "big.rs", "targets": [{"path": "a.rs", "symbols": ["A"]}]}
        ]}"#;
    let plan = parse_plan(json).unwrap();
    assert_eq!(plan.operations.len(), 45);
}

/// Canonical plan field is `selector` (matches CLI help). Alias `key` must
/// still parse so agents that emit the LLM-prior field name do not fail.
#[test]
fn parse_doc_ops_with_selector_field() {
    let json = r#"{"version": 1, "operations": [
            {"op": "doc.set", "path": "f.json", "selector": "a.b", "value": 1},
            {"op": "doc.delete", "path": "f.json", "selector": "a.b"},
            {"op": "doc.append", "path": "f.json", "selector": "arr", "value": 1},
            {"op": "doc.prepend", "path": "f.json", "selector": "arr", "value": 0},
            {"op": "doc.update", "path": "f.json", "selector": "a.b", "value": 2},
            {"op": "doc.ensure", "path": "f.json", "selector": "a.b", "value": 1},
            {"op": "doc.delete_where", "path": "f.json", "selector": "arr", "predicate": "x=1"}
        ]}"#;
    let plan = parse_plan(json).unwrap();
    assert_eq!(plan.operations.len(), 7);
    if let Operation::DocSet { selector, .. } = &plan.operations[0] {
        assert_eq!(selector, "a.b");
    } else {
        panic!("expected DocSet");
    }
}

/// Agents often emit `key` (LLM prior); alias must map it onto `selector`.
#[test]
fn parse_doc_ops_with_key_alias() {
    let json = r#"{"version": 1, "operations": [
            {"op": "doc.set", "path": "f.json", "key": "a.b", "value": 1},
            {"op": "doc.delete", "path": "f.json", "key": "a.b"},
            {"op": "doc.append", "path": "f.json", "key": "arr", "value": 1},
            {"op": "doc.prepend", "path": "f.json", "key": "arr", "value": 0},
            {"op": "doc.update", "path": "f.json", "key": "a.b", "value": 2},
            {"op": "doc.ensure", "path": "f.json", "key": "a.b", "value": 1},
            {"op": "doc.delete_where", "path": "f.json", "key": "arr", "predicate": "x=1"}
        ]}"#;
    let plan = parse_plan(json).unwrap();
    assert_eq!(plan.operations.len(), 7);
    if let Operation::DocSet {
        selector, value, ..
    } = &plan.operations[0]
    {
        assert_eq!(selector, "a.b");
        assert_eq!(value, &serde_json::json!(1));
    } else {
        panic!("expected DocSet from key alias");
    }
}

/// Agents often emit `from`/`to` for replace (LLM prior); aliases map to old/new.
#[test]
fn parse_replace_ops_with_from_to_aliases() {
    let json = r#"{"version": 1, "operations": [
            {"op": "replace", "path": "VERSION", "from": "v1", "to": "v2"}
        ]}"#;
    let plan = parse_plan(json).unwrap();
    if let Operation::Replace {
        old,
        new_text,
        path,
        ..
    } = &plan.operations[0]
    {
        assert_eq!(old, "v1");
        assert_eq!(new_text.as_deref(), Some("v2"));
        assert_eq!(path.as_deref(), Some("VERSION"));
    } else {
        panic!("expected Replace from from/to aliases");
    }
}

/// Canonical ast.rename fields are `old`/`new` (same as replace / ast.replace).
#[cfg(feature = "ast")]
#[test]
fn parse_ast_rename_with_old_new() {
    let json = r#"{"version": 1, "operations": [
            {"op": "ast.rename", "path": "lib.rs", "old": "Foo", "new": "Bar"}
        ]}"#;
    let plan = parse_plan(json).unwrap();
    if let Operation::AstRename { path, old, new, .. } = &plan.operations[0] {
        assert_eq!(path, "lib.rs");
        assert_eq!(old, "Foo");
        assert_eq!(new, "Bar");
    } else {
        panic!("expected AstRename with old/new");
    }
}

/// Legacy plan keys old_name/new_name are not co-equal API (consistency rename).
#[cfg(feature = "ast")]
#[test]
fn parse_ast_rename_rejects_legacy_old_name_fields() {
    let json = r#"{"version": 1, "operations": [
            {"op": "ast.rename", "path": "lib.rs", "old_name": "Foo", "new_name": "Bar"}
        ]}"#;
    let err = parse_plan(json).unwrap_err().to_string();
    assert!(
        err.contains("old") || err.contains("missing field"),
        "expected missing field `old` (or similar), got: {err}"
    );
}

#[test]
fn parse_plan_with_for_each() {
    let json = r#"{
            "version": 1,
            "for_each": {
                "glob": "src/**/*.rs",
                "exclude": ["src/main.rs"],
                "filter": "has_symbol(tests)"
            },
            "operations": [{"op": "replace", "path": "{path}", "old": "a", "new": "b"}]
        }"#;
    let plan = parse_plan(json).unwrap();
    let fe = plan.for_each.unwrap();
    assert_eq!(fe.glob, "src/**/*.rs");
    assert_eq!(fe.exclude, vec!["src/main.rs"]);
    assert_eq!(fe.filter.as_deref(), Some("has_symbol(tests)"));
}

#[test]
fn parse_plan_without_for_each_is_none() {
    let json = r#"{"version": 1, "operations": [{"op": "replace", "old": "a", "new": "b"}]}"#;
    let plan = parse_plan(json).unwrap();
    assert!(plan.for_each.is_none());
}

#[cfg(feature = "cli")]
#[test]
fn for_each_escape_preserves_literal_braces() {
    // When a template value contains `{{path}}`, the doubled braces should
    // produce a literal `{path}` in the output, not get substituted.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello").unwrap();

    let json = r#"{
            "version": 1,
            "for_each": { "glob": "*.txt" },
            "operations": [
                {"op": "replace", "path": "{path}", "old": "hello", "new": "{{path}} is literal"}
            ]
        }"#;
    let mut plan = parse_plan(json).unwrap();
    expand_for_each(&mut plan, dir.path()).unwrap();

    assert_eq!(plan.operations.len(), 1);
    // The `path` field should be the actual file path (substituted).
    // The `to` field should contain a literal `{path}`, NOT the file path.
    let op_json = serde_json::to_string(&plan.operations[0]).unwrap();
    assert!(
        op_json.contains(r#"{path} is literal"#),
        "escaped braces should produce literal {{path}}: {op_json}"
    );
    assert!(
        !op_json.contains("a.txt is literal"),
        "escaped braces should NOT be substituted: {op_json}"
    );
}

#[cfg(feature = "cli")]
#[test]
fn for_each_escape_mixed_literal_and_template() {
    // Mix of template variables and escaped braces in the same value.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("test.rs"), "x").unwrap();

    let json = r#"{
            "version": 1,
            "for_each": { "glob": "*.rs" },
            "operations": [
                {"op": "replace", "path": "{path}", "old": "x", "new": "file={{stem}}.{{ext}}"}
            ]
        }"#;
    let mut plan = parse_plan(json).unwrap();
    expand_for_each(&mut plan, dir.path()).unwrap();

    let op_json = serde_json::to_string(&plan.operations[0]).unwrap();
    // `{stem}` and `{ext}` should become literal, not substituted
    assert!(
        op_json.contains("file={stem}.{ext}"),
        "escaped template vars should be literal: {op_json}"
    );
}

#[cfg(feature = "cli")]
#[test]
fn for_each_unescaped_braces_still_substitute() {
    // Verify that normal (unescaped) template variables still work.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.txt"), "x").unwrap();

    let json = r#"{
            "version": 1,
            "for_each": { "glob": "*.txt" },
            "operations": [
                {"op": "replace", "path": "{path}", "old": "x", "new": "{stem}-{ext}"}
            ]
        }"#;
    let mut plan = parse_plan(json).unwrap();
    expand_for_each(&mut plan, dir.path()).unwrap();

    let op_json = serde_json::to_string(&plan.operations[0]).unwrap();
    assert!(
        op_json.contains("hello-txt"),
        "unescaped vars should substitute: {op_json}"
    );
}

#[test]
fn parse_plan_with_format_steps() {
    let json = r#"{
            "version": 1,
            "operations": [],
            "format": [{"cmd": "cargo fmt"}],
            "validate": [{"cmd": "make check"}]
        }"#;
    let plan = parse_plan(json).unwrap();
    let fmt = plan.format.unwrap();
    assert_eq!(fmt.len(), 1);
    assert_eq!(fmt[0].cmd, "cargo fmt");
}

#[test]
fn format_step_accepts_command_alias() {
    let json = r#"{
            "version": 1,
            "operations": [],
            "format": [{"command": "cargo fmt"}],
            "validate": [{"command": "make check", "required": true}]
        }"#;
    let plan = parse_plan(json).unwrap();
    let fmt = plan.format.unwrap();
    assert_eq!(fmt[0].cmd, "cargo fmt");
    let val = plan.validate.unwrap();
    assert_eq!(val[0].cmd, "make check");
    assert_eq!(val[0].required, Some(true));
}

#[test]
fn format_step_command_alias_yaml() {
    let yaml = "version: 1\noperations: []\nformat:\n  - command: cargo fmt\nvalidate:\n  - command: make check\n";
    let plan = parse_plan_yaml(yaml).unwrap();
    assert_eq!(plan.format.unwrap()[0].cmd, "cargo fmt");
    assert_eq!(plan.validate.unwrap()[0].cmd, "make check");
}

// ── YAML / TOML / auto-detect ─────────────────────────────────

#[test]
fn parse_plan_yaml_basic() {
    let yaml = "version: 1\noperations:\n  - op: replace\n    old: old\n    new: new\n";
    let plan = parse_plan_yaml(yaml).unwrap();
    assert_eq!(plan.operations.len(), 1);
    assert!(matches!(
        &plan.operations[0],
        Operation::Replace { old, new_text, .. } if old == "old" && new_text.as_deref() == Some("new")
    ));
}

#[test]
fn parse_plan_toml_basic() {
    let toml = "version = 1\n\n[[operations]]\nop = \"replace\"\nold = \"old\"\nnew = \"new\"\n";
    let plan = parse_plan_toml(toml).unwrap();
    assert_eq!(plan.operations.len(), 1);
    assert!(matches!(
        &plan.operations[0],
        Operation::Replace { old, new_text, .. } if old == "old" && new_text.as_deref() == Some("new")
    ));
}

#[test]
fn parse_plan_auto_detects_yaml() {
    let yaml = "version: 1\noperations:\n  - op: replace\n    old: a\n    new: b\n";
    let plan = parse_plan_auto(yaml, Some("plan.yaml"), None).unwrap();
    assert_eq!(plan.operations.len(), 1);
}

#[test]
fn parse_plan_auto_format_hint_overrides_extension() {
    let yaml = "version: 1\noperations:\n  - op: replace\n    old: a\n    new: b\n";
    // Extension says .json but hint says yaml.
    let plan = parse_plan_auto(yaml, Some("plan.json"), Some("yaml")).unwrap();
    assert_eq!(plan.operations.len(), 1);
}

#[test]
fn parse_plan_auto_defaults_to_json() {
    let json = r#"{"version": 1, "operations": [{"op": "replace", "old": "a", "new": "b"}]}"#;
    let plan = parse_plan_auto(json, Some("plan.txt"), None).unwrap();
    assert_eq!(plan.operations.len(), 1);
}

#[test]
fn parse_plan_defaults_strict_when_omitted() {
    let json = r#"{"version": 1, "operations": [{"op": "replace", "old": "a", "new": "b"}]}"#;
    let plan = parse_plan(json).unwrap();
    assert_eq!(plan.strict, None);
    assert!(effective_strict(plan.strict, None, false));
    assert!(!effective_strict(plan.strict, None, true));
    assert!(!effective_strict(Some(true), None, true));
    assert!(!effective_strict(None, Some(false), false));
    assert!(effective_strict(Some(true), Some(false), false));
}

#[test]
fn parse_plan_strict_and_all_policy_fields() {
    let json = r#"{
            "version": 1,
            "strict": true,
            "write_policy": {
                "ensure_final_newline": true,
                "normalize_eol": "crlf",
                "trim_trailing_whitespace": true,
                "collapse_blanks": true
            },
            "operations": [],
            "format": [{"cmd": "fmt", "timeout": 30}],
            "validate": [{"cmd": "check", "required": true, "timeout": 120}]
        }"#;
    let plan = parse_plan(json).unwrap();
    assert_eq!(plan.strict, Some(true));
    let wp = plan.write_policy.unwrap();
    assert_eq!(wp.ensure_final_newline, Some(true));
    assert_eq!(wp.normalize_eol.as_deref(), Some("crlf"));
    assert_eq!(wp.trim_trailing_whitespace, Some(true));
    assert_eq!(wp.collapse_blanks, Some(true));
    let fmt = &plan.format.unwrap()[0];
    assert_eq!(fmt.timeout, Some(30));
    let val = &plan.validate.unwrap()[0];
    assert_eq!(val.required, Some(true));
    assert_eq!(val.timeout, Some(120));
}

#[test]
fn declared_paths_covers_operation_variants() {
    // Replace with path + glob (both collected for guard)
    let json = r#"{"version": 1,"operations":[{"op":"replace","path":"src/main.rs","glob":"**/*.rs","old":"old","new":"new"}]}"#;
    let plan = parse_plan(json).unwrap();
    let ps = declared_paths(&plan.operations[0]);
    assert!(ps.contains(&"src/main.rs".to_string()) && ps.contains(&"**/*.rs".to_string()));

    // FileRename (cross-file paths)
    let json = r#"{"version": 1,"operations":[{"op":"file.rename","from":"old.txt","to":"new.txt","force":false}]}"#;
    let plan = parse_plan(json).unwrap();
    assert_eq!(
        declared_paths(&plan.operations[0]),
        vec!["old.txt", "new.txt"]
    );

    // MdMoveSection same-file (to omitted)
    let json = r#"{"version": 1,"operations":[{"op":"md.move_section","path":"doc.md","heading":"Section"}]}"#;
    let plan = parse_plan(json).unwrap();
    assert_eq!(declared_paths(&plan.operations[0]), vec!["doc.md"]);

    // MdMoveSection cross-file
    let json = r#"{"version": 1,"operations":[{"op":"md.move_section","path":"src.md","heading":"H","to":"dst.md"}]}"#;
    let plan = parse_plan(json).unwrap();
    let ps = declared_paths(&plan.operations[0]);
    assert!(ps.contains(&"src.md".to_string()) && ps.contains(&"dst.md".to_string()));

    // PatchApply: now parses diff and returns file paths
    let json = r#"{"version": 1,"operations":[{"op":"patch.apply","diff":"--- a/x\n+++ b/x\n@@ -1 +1 @@\n- old\n+ new\n"}]}"#;
    let plan = parse_plan(json).unwrap();
    assert_eq!(declared_paths(&plan.operations[0]), vec!["x"]);

    // PatchApply with invalid diff: returns empty (error deferred to apply time)
    let json = r#"{"version": 1,"operations":[{"op":"patch.apply","diff":"not a valid diff"}]}"#;
    let plan = parse_plan(json).unwrap();
    assert!(declared_paths(&plan.operations[0]).is_empty());

    // Representative single-path ops
    let json = r#"{"version": 1,"operations":[{"op":"doc.set","path":"c.json","selector":"v","value":42}]}"#;
    let plan = parse_plan(json).unwrap();
    assert_eq!(declared_paths(&plan.operations[0]), vec!["c.json"]);

    let json = r#"{"version": 1,"operations":[{"op":"read","path":"f.txt"}]}"#;
    let plan = parse_plan(json).unwrap();
    assert_eq!(declared_paths(&plan.operations[0]), vec!["f.txt"]);
}

#[test]
fn op_to_doc_mutation_covers_all_doc_variants() {
    use crate::ops::doc::DocMutation;

    let cases = [
        r#"{"op":"doc.set","path":"f.json","selector":"k","value":1}"#,
        r#"{"op":"doc.delete","path":"f.json","selector":"k"}"#,
        r#"{"op":"doc.merge","path":"f.json","value":{}}"#,
        r#"{"op":"doc.append","path":"f.json","selector":"arr","value":1}"#,
        r#"{"op":"doc.prepend","path":"f.json","selector":"arr","value":0}"#,
        r#"{"op":"doc.update","path":"f.json","selector":"k","value":2}"#,
        r#"{"op":"doc.move","path":"f.json","from":"a","to":"b"}"#,
        r#"{"op":"doc.ensure","path":"f.json","selector":"k","value":1}"#,
        r#"{"op":"doc.delete_where","path":"f.json","selector":"arr","predicate":"n=x"}"#,
    ];

    for (i, case) in cases.iter().enumerate() {
        let json = format!(r#"{{"version": 1,"operations":[{case}]}}"#);
        let plan = parse_plan(&json).unwrap();
        let result = op_to_doc_mutation(&plan.operations[0]);
        assert!(
            result.is_some(),
            "doc variant {i} should return Some, got None"
        );
        let (path, _mutation) = result.unwrap();
        assert_eq!(path, "f.json", "variant {i} path mismatch");
    }

    // Non-doc variants return None
    let non_doc = r#"{"version": 1,"operations":[{"op":"replace","old":"a","new":"b"}]}"#;
    let plan = parse_plan(non_doc).unwrap();
    assert!(op_to_doc_mutation(&plan.operations[0]).is_none());

    // Verify the specific mutation variant matches
    let set_json = r#"{"version": 1,"operations":[{"op":"doc.set","path":"x.json","selector":"key","value":"val"}]}"#;
    let plan = parse_plan(set_json).unwrap();
    let (_, mutation) = op_to_doc_mutation(&plan.operations[0]).unwrap();
    assert!(matches!(mutation, DocMutation::Set { .. }));
}

/// Regression: FileCreate, FileDelete, and FileRename must trigger a doc
/// cache flush, otherwise a preceding doc.set can be silently undone.
#[test]
fn needs_doc_flush_includes_file_create_delete_rename() {
    let create = Operation::FileCreate {
        path: "f.json".into(),
        content: "{}".into(),
        force: Some(false),
    };
    assert!(
        create.needs_doc_flush(),
        "FileCreate must trigger doc flush"
    );

    let delete = Operation::FileDelete {
        path: "f.json".into(),
    };
    assert!(
        delete.needs_doc_flush(),
        "FileDelete must trigger doc flush"
    );

    let rename = Operation::FileRename {
        from: "a.json".into(),
        to: "b.json".into(),
        force: false,
    };
    assert!(
        rename.needs_doc_flush(),
        "FileRename must trigger doc flush"
    );
}
