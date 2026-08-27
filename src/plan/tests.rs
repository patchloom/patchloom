use super::*;

#[cfg(feature = "files")]
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

#[cfg(feature = "files")]
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

#[cfg(feature = "files")]
#[test]
fn substitute_single_pass_basic() {
    let template = "file: {path}, dir: {dir}";
    let vars: &[(&str, String)] = &[("{path}", "src/main.rs".into()), ("{dir}", "src".into())];
    let result = substitute_single_pass(template, vars);
    assert_eq!(result, "file: src/main.rs, dir: src");
}

/// Regression: substitute_single_pass must handle multi-byte UTF-8
/// characters correctly (not corrupt them via byte-as-char casting).
#[cfg(feature = "files")]
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

#[cfg(feature = "ast")]
#[test]
fn ast_rename_accepts_from_to_aliases() {
    let v: serde_json::Value = serde_json::json!({
        "op": "ast.rename",
        "path": "src/lib.rs",
        "from": "OldName",
        "to": "NewName"
    });
    let op: Operation = serde_json::from_value(v).expect("from/to should alias old/new");
    match op {
        Operation::AstRename { old, new, .. } => {
            assert_eq!(old, "OldName");
            assert_eq!(new, "NewName");
        }
        other => panic!("expected AstRename, got {other:?}"),
    }
}

#[cfg(feature = "ast")]
#[test]
fn ast_rewrite_signature_accepts_from_to_aliases() {
    let v: serde_json::Value = serde_json::json!({
        "op": "ast.rewrite_signature",
        "path": "src/lib.rs",
        "from": "process",
        "to": "fn process(x: i32) -> String"
    });
    let op: Operation = serde_json::from_value(v).expect("from/to aliases");
    match op {
        Operation::AstRewriteSignature {
            old, new_signature, ..
        } => {
            assert_eq!(old, "process");
            assert_eq!(
                new_signature.as_deref(),
                Some("fn process(x: i32) -> String")
            );
        }
        other => panic!("expected AstRewriteSignature, got {other:?}"),
    }
}

#[test]
fn apply_fragment_accepts_new_alias_for_fragment() {
    // Agents often copy replace_text priors (`new`/`to`) onto apply.fragment.
    let v: serde_json::Value = serde_json::json!({
        "op": "apply.fragment",
        "path": "f.rs",
        "old": "anchor",
        "new": "replacement"
    });
    let op: Operation = serde_json::from_value(v).expect("new should alias fragment");
    match op {
        Operation::ApplyFragment { fragment, old, .. } => {
            assert_eq!(fragment, "replacement");
            assert_eq!(old.as_deref(), Some("anchor"));
        }
        other => panic!("expected ApplyFragment, got {other:?}"),
    }
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
    parse_plan(json).expect_err("expected error");
}

#[test]
fn parse_plan_missing_operations_fails() {
    let json = r#"{"version": 1, "cwd": "/tmp"}"#;
    parse_plan(json).expect_err("expected error");
}

#[test]
fn parse_plan_ops_alias_accepted() {
    // Agents often emit "ops" instead of "operations".
    let json =
        r#"{"version": 1, "ops": [{"op": "replace", "path": "f.txt", "old": "a", "new": "b"}]}"#;
    let plan = parse_plan(json).expect("ops alias should parse");
    assert_eq!(plan.operations.len(), 1);
}

/// Agents invent `md.insert_before_section` by symmetry with insert_after_section.
#[test]
fn parse_md_insert_before_section_alias() {
    let json = r#"{"version":1,"operations":[{"op":"md.insert_before_section","path":"f.md","heading":"H","content":"x"}]}"#;
    let plan = parse_plan(json).expect("insert_before_section alias should parse");
    assert!(
        matches!(
            plan.operations[0],
            crate::plan::Operation::MdInsertBeforeHeading { .. }
        ),
        "alias must map to MdInsertBeforeHeading"
    );
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
            {"op": "search", "path": ".", "pattern": "foo", "literal": true, "exclude_patterns": ["target/**"], "custom_ignore_filenames": [".agentignore"], "max_results": 10},
            {"op": "ast.rename", "path": "f.rs", "old": "Foo", "new": "Bar"},
            {"op": "ast.replace", "path": "f.rs", "symbol": "main", "old": "a", "new": "b"},
            {"op": "ast.insert", "path": "f.rs", "content": "fn new() {}", "after": "main"},
            {"op": "ast.wrap", "path": "f.rs", "symbols": ["helper"], "wrapper": "mod internal"},
            {"op": "ast.imports", "path": "f.rs", "add": ["use std::io;"]},
            {"op": "ast.reorder", "path": "f.rs", "order": "alphabetical"},
            {"op": "ast.reorder", "path": "f.rs", "order": ["b", "a"], "inside": "mod tests"},
            {"op": "ast.group", "path": "f.rs", "module": "tests", "symbols": ["test_a"]},
            {"op": "ast.move", "path": "src.rs", "target": "dst.rs", "symbols": ["foo"]},
            {"op": "ast.move", "path": "src.rs", "target": "dst.rs", "symbols": ["foo"], "update_imports": true, "old_module_path": "crate::old_mod", "new_module_path": "crate::new_mod"},
            {"op": "ast.extract_to_file", "source": "lib.rs", "symbol": "tests", "target": "lib_tests.rs"},
            {"op": "ast.split", "source": "big.rs", "targets": [{"path": "a.rs", "symbols": ["A"]}]}
        ]}"#;
    let plan = parse_plan(json).unwrap();
    assert_eq!(plan.operations.len(), 46);
    let found = plan.operations.iter().any(|op| {
        matches!(
            op,
            Operation::AstMove {
                update_imports: true,
                old_module_path: Some(old),
                new_module_path: Some(new),
                ..
            } if old == "crate::old_mod" && new == "crate::new_mod"
        )
    });
    assert!(found, "ast.move update_imports fields should parse");
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

/// Plans without `if_exists` deserialize with the fail-hard default (#2231).
#[test]
fn doc_set_and_file_delete_if_exists_defaults_false() {
    let json = r#"{
        "version": 1,
        "operations": [
            {"op":"doc.set","path":"f.json","selector":"k","value":1},
            {"op":"file.delete","path":"x.txt"}
        ]
    }"#;
    let plan = parse_plan(json).unwrap();
    match &plan.operations[0] {
        Operation::DocSet { if_exists, .. } => {
            assert!(!*if_exists, "doc.set without if_exists must default false");
        }
        other => panic!("expected DocSet, got {other:?}"),
    }
    match &plan.operations[1] {
        Operation::FileDelete { if_exists, .. } => {
            assert!(
                !*if_exists,
                "file.delete without if_exists must default false"
            );
        }
        other => panic!("expected FileDelete, got {other:?}"),
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

#[cfg(feature = "files")]
#[test]
fn for_each_rejects_plan_cwd_combination() {
    // Glob is relative to invocation cwd; plan.cwd re-roots after expand and
    // double-prefixes {path}. MCP already rejected this; CLI/library must too.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("nested")).unwrap();
    std::fs::write(dir.path().join("nested/a.txt"), "x").unwrap();

    let json = r#"{
            "version": 1,
            "cwd": "nested",
            "for_each": { "glob": "**/*.txt" },
            "operations": [
                {"op": "replace", "path": "{path}", "old": "x", "new": "y"}
            ]
        }"#;
    let mut plan = parse_plan(json).unwrap();
    let err = expand_for_each(&mut plan, dir.path()).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("plan.cwd cannot be combined with for_each"),
        "expected cwd+for_each reject, got: {msg}"
    );
}

#[cfg(feature = "files")]
#[test]
fn for_each_rejects_multi_match_without_file_template() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}").unwrap();
    std::fs::write(dir.path().join("b.rs"), "fn b() {}").unwrap();

    let json = r#"{
            "version": 1,
            "for_each": { "glob": "*.rs" },
            "operations": [
                {"op": "file.append", "path": "CHANGELOG.md", "content": "- touch\n"}
            ]
        }"#;
    let mut plan = parse_plan(json).unwrap();
    let err = expand_for_each(&mut plan, dir.path()).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("no operation uses a file template") && msg.contains("2 files"),
        "expected multi-match fixed-path reject, got: {msg}"
    );
}

#[cfg(feature = "files")]
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

#[cfg(feature = "files")]
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

#[cfg(feature = "files")]
#[test]
fn for_each_item_alias_substitutes_path() {
    // Agents often write {item}; treat it as {path}.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.txt"), "x").unwrap();

    let json = r#"{
            "version": 1,
            "for_each": { "glob": "*.txt" },
            "operations": [
                {"op": "replace", "path": "{item}", "old": "x", "new": "y"}
            ]
        }"#;
    let mut plan = parse_plan(json).unwrap();
    expand_for_each(&mut plan, dir.path()).unwrap();
    assert_eq!(plan.operations.len(), 1);
    let op_json = serde_json::to_string(&plan.operations[0]).unwrap();
    assert!(
        op_json.contains("hello.txt"),
        "item alias should expand to path: {op_json}"
    );
    assert!(
        !op_json.contains("{item}"),
        "item placeholder must not remain: {op_json}"
    );
}

#[cfg(all(feature = "files", feature = "ast"))]
#[test]
fn for_each_has_symbol_matches_nested_method() {
    // Methods live under impl; top-level-only filter would drop the file.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("svc.rs"),
        "struct S;\nimpl S {\n    fn process_request(&self) {}\n}\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("other.rs"), "fn unrelated() {}\n").unwrap();

    let json = r#"{
            "version": 1,
            "for_each": { "glob": "*.rs", "filter": "has_symbol(process_request)" },
            "operations": [
                {"op": "replace", "path": "{path}", "old": "x", "new": "y"}
            ]
        }"#;
    let mut plan = parse_plan(json).unwrap();
    expand_for_each(&mut plan, dir.path()).unwrap();
    assert_eq!(plan.operations.len(), 1, "only the method host file");
    let op_json = serde_json::to_string(&plan.operations[0]).unwrap();
    assert!(
        op_json.contains("svc.rs"),
        "nested method filter must keep svc.rs: {op_json}"
    );
}

#[cfg(feature = "files")]
#[test]
fn for_each_unknown_path_template_is_invalid_input() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.txt"), "x").unwrap();

    let json = r#"{
            "version": 1,
            "for_each": { "glob": "*.txt" },
            "operations": [
                {"op": "replace", "path": "{file}", "old": "x", "new": "y"}
            ]
        }"#;
    let mut plan = parse_plan(json).unwrap();
    let err = expand_for_each(&mut plan, dir.path()).unwrap_err();
    assert!(
        crate::exit::is_invalid_input(&err),
        "expected InvalidInputError, got: {err:#}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("unsubstituted template") && msg.contains("{file}"),
        "message should name the bad placeholder: {msg}"
    );
}

#[cfg(feature = "files")]
#[test]
fn for_each_invalid_glob_is_invalid_input() {
    // Unclosed character class is a globset parse error. Agents must see
    // invalid_input (exit 1), not an untyped parse_error remap (exit 4).
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.txt"), "x").unwrap();

    let json = r#"{
            "version": 1,
            "for_each": { "glob": "[" },
            "operations": [
                {"op": "replace", "path": "{path}", "old": "x", "new": "y"}
            ]
        }"#;
    let mut plan = parse_plan(json).unwrap();
    let err = expand_for_each(&mut plan, dir.path()).unwrap_err();
    assert!(
        crate::exit::is_invalid_input(&err),
        "expected InvalidInputError for bad glob, got: {err:#}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("invalid glob"),
        "message should name the glob failure: {msg}"
    );
}

#[cfg(feature = "files")]
#[test]
fn for_each_invalid_exclude_glob_is_invalid_input() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.txt"), "x").unwrap();

    let json = r#"{
            "version": 1,
            "for_each": { "glob": "*.txt", "exclude": ["["] },
            "operations": [
                {"op": "replace", "path": "{path}", "old": "x", "new": "y"}
            ]
        }"#;
    let mut plan = parse_plan(json).unwrap();
    let err = expand_for_each(&mut plan, dir.path()).unwrap_err();
    assert!(
        crate::exit::is_invalid_input(&err),
        "expected InvalidInputError for bad exclude glob, got: {err:#}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("invalid exclude glob"),
        "message should name the exclude glob failure: {msg}"
    );
}

#[cfg(feature = "files")]
#[test]
fn for_each_unsupported_filter_is_invalid_input() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.txt"), "x").unwrap();

    let json = r#"{
            "version": 1,
            "for_each": { "glob": "*.txt", "filter": "contains(hello)" },
            "operations": [
                {"op": "replace", "path": "{path}", "old": "x", "new": "y"}
            ]
        }"#;
    let mut plan = parse_plan(json).unwrap();
    let err = expand_for_each(&mut plan, dir.path()).unwrap_err();
    assert!(
        crate::exit::is_invalid_input(&err),
        "expected InvalidInputError for unknown filter, got: {err:#}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("unsupported filter") && msg.contains("contains(hello)"),
        "message should name the filter expression: {msg}"
    );
}

#[cfg(feature = "files")]
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

    // PatchApply 100% copy: dest + source for PathGuard / host dest-deny (#2171)
    let json = r#"{"version": 1,"operations":[{"op":"patch.apply","diff":"diff --git a/foo.rs b/bar.rs\nsimilarity index 100%\ncopy from foo.rs\ncopy to bar.rs\n"}]}"#;
    let plan = parse_plan(json).unwrap();
    let ps = declared_paths(&plan.operations[0]);
    assert!(
        ps.contains(&"bar.rs".to_string()) && ps.contains(&"foo.rs".to_string()),
        "copy dest+source must be declared: {ps:?}"
    );

    // PatchApply Begin Patch: dests including Move to (#2219)
    let json = r#"{"version": 1,"operations":[{"op":"patch.apply","diff":"*** Begin Patch\n*** Update File: b.rs\n*** Move to: c.rs\n@@\n-old\n+new\n*** End Patch\n"}]}"#;
    let plan = parse_plan(json).unwrap();
    let ps = declared_paths(&plan.operations[0]);
    assert!(
        ps.contains(&"b.rs".to_string()) && ps.contains(&"c.rs".to_string()),
        "Begin Patch dests must be declared: {ps:?}"
    );

    // PatchApply SEARCH/REPLACE dests (#2221)
    let json = r#"{"version": 1,"operations":[{"op":"patch.apply","diff":"<<<<<<< SEARCH\nb.rs\n-------\nold\n=======\nnew\n>>>>>>> REPLACE\n"}]}"#;
    let plan = parse_plan(json).unwrap();
    assert_eq!(declared_paths(&plan.operations[0]), vec!["b.rs"]);

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
        if_exists: false,
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

#[test]
fn replace_accepts_file_alias() {
    let json =
        r#"{"version":1,"operations":[{"op":"replace","file":"README.md","old":"a","new":"b"}]}"#;
    let plan: Plan = serde_json::from_str(json).expect("deserialize");
    match &plan.operations[0] {
        Operation::Replace {
            path,
            old,
            new_text,
            ..
        } => {
            assert_eq!(path.as_deref(), Some("README.md"));
            assert_eq!(old, "a");
            assert_eq!(new_text.as_deref(), Some("b"));
        }
        other => panic!("expected Replace, got {other:?}"),
    }
}

#[test]
fn lifecycle_cmds_yields_format_then_validate() {
    let plan = parse_plan(
        r#"{
            "version": 1,
            "operations": [{"op": "file.create", "path": "ok.txt", "content": "x"}],
            "format": [{"cmd": "cargo fmt"}],
            "validate": [{"command": "true"}]
        }"#,
    )
    .unwrap();
    let cmds: Vec<&str> = lifecycle_cmds(&plan).collect();
    assert_eq!(cmds, vec!["cargo fmt", "true"]);
}

#[test]
fn refuse_lifecycle_shell_metas_allows_plain_formatters() {
    for cmd in ["true", "cargo fmt", "rustfmt", "rustfmt --edition 2021"] {
        refuse_lifecycle_shell_metas(cmd)
            .unwrap_or_else(|e| panic!("{cmd:?} should be allowed: {e}"));
    }
}

#[test]
fn refuse_lifecycle_shell_metas_rejects_redirects_and_substitutions() {
    let cases = [
        "printf secret > /tmp/escape.env",
        "printf secret >> /tmp/escape.env",
        "cat .env | sh",
        "true && rm -rf /",
        "true || echo x",
        "true; echo x",
        "echo $(whoami)",
        "echo ${HOME}",
        "echo `whoami`",
        "echo x\necho y",
        "echo x\recho y",
    ];
    for cmd in cases {
        let err = refuse_lifecycle_shell_metas(cmd).expect_err(cmd);
        assert_eq!(
            crate::fallback::edit_error_kind(&err),
            Some(crate::fallback::EditErrorKind::InvalidInput),
            "expected invalid_input for {cmd:?}: {err}"
        );
        assert!(
            err.to_string().contains("metacharacter"),
            "expected metacharacter diagnostic for {cmd:?}: {err}"
        );
    }
}

#[cfg(feature = "files")]
#[test]
fn for_each_expand_declared_paths_are_concrete() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("notes.txt"), "x\n").unwrap();
    std::fs::write(dir.path().join("keep.txt"), "y\n").unwrap();
    let mut plan = parse_plan(
        r#"{
            "version": 1,
            "for_each": {"glob": "*.txt"},
            "operations": [{"op": "file.create", "path": "{path}", "content": "z", "force": true}]
        }"#,
    )
    .unwrap();
    expand_for_each(&mut plan, dir.path()).unwrap();
    assert!(plan.for_each.is_none());
    let mut paths: Vec<String> = plan
        .operations
        .iter()
        .flat_map(Operation::declared_paths)
        .collect();
    paths.sort();
    assert_eq!(paths, vec!["keep.txt".to_string(), "notes.txt".to_string()]);
    assert!(
        paths.iter().all(|p| !p.contains('{')),
        "declared_paths after expand must not keep {{path}} templates: {paths:?}"
    );
}

#[cfg(feature = "files")]
#[test]
fn for_each_exclude_omits_vendor_tree() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("vendor/pkg")).unwrap();
    std::fs::write(dir.path().join("app.js"), "a\n").unwrap();
    std::fs::write(dir.path().join("vendor/pkg/lib.js"), "v\n").unwrap();
    let mut plan = parse_plan(
        r#"{
            "version": 1,
            "for_each": {"glob": "**/*.js", "exclude": ["vendor/**"]},
            "operations": [{"op": "replace", "path": "{path}", "old": "a", "new": "b"}]
        }"#,
    )
    .unwrap();
    expand_for_each(&mut plan, dir.path()).unwrap();
    let paths: Vec<String> = plan
        .operations
        .iter()
        .flat_map(Operation::declared_paths)
        .collect();
    assert_eq!(paths.len(), 1, "only app.js: {paths:?}");
    assert!(
        paths.iter().any(|p| p.ends_with("app.js")),
        "kept app.js: {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.contains("vendor")),
        "vendor/** excluded: {paths:?}"
    );
}
