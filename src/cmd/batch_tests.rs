use super::*;

mod basic {
    use super::*;

    #[test]
    fn tokenize_simple() {
        let tokens = tokenize("doc.set config.json version 42").unwrap();
        assert_eq!(tokens, vec!["doc.set", "config.json", "version", "42"]);
    }

    #[test]
    fn tokenize_quoted() {
        let tokens = tokenize(r#"doc.set config.json key "hello world""#).unwrap();
        assert_eq!(tokens, vec!["doc.set", "config.json", "key", "hello world"]);
    }

    #[test]
    fn tokenize_escaped_quote() {
        let tokens = tokenize(r#"replace f.txt "say \"hi\"" "say \"bye\"""#).unwrap();
        assert_eq!(
            tokens,
            vec!["replace", "f.txt", r#"say "hi""#, r#"say "bye""#]
        );
    }

    #[test]
    fn tokenize_json_value_unquoted() {
        // Unquoted JSON without internal quotes works fine.
        let tokens = tokenize("doc.set f.json key 42").unwrap();
        assert_eq!(tokens, vec!["doc.set", "f.json", "key", "42"]);
    }

    #[test]
    fn tokenize_json_value_quoted() {
        // JSON objects with internal quotes must be double-quoted.
        let tokens = tokenize(r#"doc.merge f.json "{\"nested\":\"value\",\"num\":42}""#).unwrap();
        assert_eq!(
            tokens,
            vec!["doc.merge", "f.json", r#"{"nested":"value","num":42}"#]
        );
    }

    #[test]
    fn parse_json_value_number() {
        let v = parse_json_value("42").unwrap();
        assert_eq!(v, serde_json::json!(42));
    }

    #[test]
    fn parse_json_value_string_fallback() {
        let v = parse_json_value("hello").unwrap();
        assert_eq!(v, serde_json::json!("hello"));
    }

    #[test]
    fn parse_json_value_object() {
        let v = parse_json_value(r#"{"a":1}"#).unwrap();
        assert_eq!(v, serde_json::json!({"a": 1}));
    }

    #[test]
    fn parse_json_value_quoted_string() {
        let v = parse_json_value(r#""2.0.0""#).unwrap();
        assert_eq!(v, serde_json::json!("2.0.0"));
    }

    #[test]
    fn parse_line_doc_set() {
        let op = parse_line("doc.set config.json version 42", 1).unwrap();
        assert!(matches!(
            op,
            Operation::DocSet { path, selector, value }
            if path == "config.json" && selector == "version" && value == serde_json::json!(42)
        ));
    }

    #[test]
    fn parse_line_doc_set_string_value() {
        let op = parse_line(r#"doc.set config.json name "my app""#, 1).unwrap();
        assert!(matches!(
            op,
            Operation::DocSet { path, selector, value }
            if path == "config.json" && selector == "name" && value == serde_json::json!("my app")
        ));
    }

    #[test]
    fn parse_line_replace() {
        let op = parse_line(r#"replace src/main.rs "old text" "new text""#, 1).unwrap();
        assert!(matches!(
            op,
            Operation::Replace { path: Some(p), from, to: Some(t), .. }
            if p == "src/main.rs" && from == "old text" && t == "new text"
        ));
    }

    #[test]
    fn parse_line_file_create() {
        let op = parse_line(r#"file.create hello.txt "Hello, World!""#, 1).unwrap();
        assert!(matches!(
            op,
            Operation::FileCreate { path, content, .. }
            if path == "hello.txt" && content == "Hello, World!"
        ));
    }

    #[test]
    fn parse_line_file_delete() {
        let op = parse_line("file.delete old.txt", 1).unwrap();
        assert!(matches!(op, Operation::FileDelete { path } if path == "old.txt"));
    }

    #[test]
    fn parse_line_tidy_fix() {
        let op = parse_line("tidy.fix src/lib.rs", 1).unwrap();
        assert!(matches!(
            op,
            Operation::TidyFix { path, ensure_final_newline, trim_trailing_whitespace, normalize_eol }
            if path == "src/lib.rs"
                && ensure_final_newline.is_none()
                && trim_trailing_whitespace.is_none()
                && normalize_eol.is_none()
        ));
    }

    #[test]
    fn parse_line_md_upsert_bullet() {
        let input = "md.upsert_bullet AGENTS.md \"## Rules\" \"- New rule\"";
        let op = parse_line(input, 1).unwrap();
        assert!(matches!(
            op,
            Operation::MdUpsertBullet { path, heading, bullet }
            if path == "AGENTS.md" && heading == "## Rules" && bullet == "- New rule"
        ));
    }

    #[test]
    fn parse_line_doc_update() {
        // JSON objects with internal quotes must be escaped inside double quotes
        // in batch format (see tokenize_json_value_quoted test).
        let op = parse_line(r#"doc.update config.json items[*] "{\"active\":true}""#, 1).unwrap();
        assert!(matches!(
            op,
            Operation::DocUpdate { path, selector, value }
            if path == "config.json" && selector == "items[*]" && value == serde_json::json!({"active": true})
        ));
    }

    #[test]
    fn parse_line_doc_move() {
        let op = parse_line(r#"doc.move config.json old_key new_key"#, 1).unwrap();
        assert!(matches!(
            op,
            Operation::DocMove { path, from, to }
            if path == "config.json" && from == "old_key" && to == "new_key"
        ));
    }

    #[test]
    fn parse_line_doc_delete_where() {
        let op = parse_line(r#"doc.delete_where config.json items "status=obsolete""#, 1).unwrap();
        assert!(matches!(
            op,
            Operation::DocDeleteWhere { path, selector, predicate }
            if path == "config.json" && selector == "items" && predicate == "status=obsolete"
        ));
    }

    #[test]
    fn parse_line_md_replace_section() {
        let op = parse_line("md.replace_section README.md \"## API\" \"New content\"", 1).unwrap();
        assert!(matches!(
            op,
            Operation::MdReplaceSection { path, heading, content }
            if path == "README.md" && heading == "## API" && content == "New content"
        ));
    }

    #[test]
    fn parse_line_md_insert_after_heading() {
        let op = parse_line(
            "md.insert_after_heading README.md \"## Rules\" \"New paragraph\"",
            1,
        )
        .unwrap();
        assert!(matches!(
            op,
            Operation::MdInsertAfterHeading { path, heading, content }
            if path == "README.md" && heading == "## Rules" && content == "New paragraph"
        ));
    }

    #[test]
    fn parse_line_md_insert_before_heading() {
        let op = parse_line(
            "md.insert_before_heading README.md \"## Rules\" \"Preamble\"",
            1,
        )
        .unwrap();
        assert!(matches!(
            op,
            Operation::MdInsertBeforeHeading { path, heading, content }
            if path == "README.md" && heading == "## Rules" && content == "Preamble"
        ));
    }

    #[test]
    fn parse_line_md_dedupe_headings() {
        let op = parse_line("md.dedupe_headings CHANGELOG.md", 1).unwrap();
        assert!(matches!(
            op,
            Operation::MdDedupeHeadings { path }
            if path == "CHANGELOG.md"
        ));
    }

    #[test]
    fn parse_line_md_lint_agents() {
        let op = parse_line("md.lint_agents AGENTS.md", 1).unwrap();
        assert!(matches!(
            op,
            Operation::MdLintAgents { path }
            if path == "AGENTS.md"
        ));
    }

    #[test]
    fn parse_line_doc_delete() {
        let op = parse_line("doc.delete config.json old_key", 1).unwrap();
        assert!(matches!(op, Operation::DocDelete { ref path, ref selector }
            if path == "config.json" && selector == "old_key"));
    }

    #[test]
    fn parse_line_doc_merge() {
        let op = parse_line(r#"doc.merge config.json "{\"debug\":true}""#, 1).unwrap();
        assert!(matches!(op, Operation::DocMerge { ref path, ref value }
                if path == "config.json" && value == &serde_json::json!({"debug": true})));
    }

    #[test]
    fn parse_line_doc_ensure() {
        let op = parse_line(r#"doc.ensure config.json version "beta""#, 1).unwrap();
        assert!(
            matches!(op, Operation::DocEnsure { ref path, ref selector, ref value }
            if path == "config.json" && selector == "version" && value == &serde_json::json!("beta"))
        );
    }

    #[test]
    fn parse_line_doc_append() {
        let op = parse_line(r#"doc.append config.json tags "new""#, 1).unwrap();
        assert!(
            matches!(op, Operation::DocAppend { ref path, ref selector, ref value }
            if path == "config.json" && selector == "tags" && value == &serde_json::json!("new"))
        );
    }

    #[test]
    fn parse_line_doc_prepend() {
        let op = parse_line(r#"doc.prepend config.json items "first""#, 1).unwrap();
        assert!(
            matches!(op, Operation::DocPrepend { ref path, ref selector, ref value }
            if path == "config.json" && selector == "items" && value == &serde_json::json!("first"))
        );
    }

    #[test]
    fn parse_line_md_table_append() {
        let input = "md.table_append README.md \"## Commands\" \"| new | desc |\"";
        let op = parse_line(input, 1).unwrap();
        assert!(
            matches!(op, Operation::MdTableAppend { ref path, ref heading, ref row }
            if path == "README.md" && heading == "## Commands" && row == "| new | desc |")
        );
    }

    #[test]
    fn parse_line_file_rename() {
        let op = parse_line("file.rename old.txt new.txt", 1).unwrap();
        assert!(
            matches!(op, Operation::FileRename { ref from, ref to, force }
            if from == "old.txt" && to == "new.txt" && !force)
        );
    }

    #[test]
    fn full_batch_parse() {
        let input = r#"
# Update versions across the project
doc.set package.json version "2.0.0"
doc.set config.yaml app.version "2.0.0"
replace README.md "1.0.0" "2.0.0"

# Create a new file
file.create hello.txt "Hello, World!"
"#;
        let mut operations = Vec::new();
        for (i, line) in input.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            operations.push(parse_line(trimmed, i + 1).unwrap());
        }
        assert_eq!(operations.len(), 4);
    }

    #[test]
    fn parse_line_md_move_section_same_file() {
        let op = parse_line("md.move_section README.md \"FAQ\" before \"License\"", 1).unwrap();
        assert!(matches!(
            op,
            Operation::MdMoveSection { path, heading, to, before, after }
            if path == "README.md" && heading == "FAQ" && to.is_none()
               && before.as_deref() == Some("License") && after.is_none()
        ));
    }

    #[test]
    fn parse_line_md_move_section_cross_file() {
        let op = parse_line(
            "md.move_section spec.md \"Appendix\" dest.md after \"Layer 4\"",
            1,
        )
        .unwrap();
        assert!(matches!(
            op,
            Operation::MdMoveSection { path, heading, to, before, after }
            if path == "spec.md" && heading == "Appendix"
               && to.as_deref() == Some("dest.md")
               && before.is_none() && after.as_deref() == Some("Layer 4")
        ));
    }
}

mod edge_cases {
    use super::*;

    #[test]
    fn tokenize_empty_quoted_string() {
        let tokens = tokenize(r#"doc.set f.json key """#).unwrap();
        assert_eq!(tokens, vec!["doc.set", "f.json", "key", ""]);
    }

    #[test]
    fn tokenize_empty_quoted_string_mid_line() {
        let tokens = tokenize(r#"replace f.txt "" "new""#).unwrap();
        assert_eq!(tokens, vec!["replace", "f.txt", "", "new"]);
    }
}

mod error_handling {
    use super::*;

    #[test]
    fn tokenize_trailing_backslash_error() {
        let err = tokenize(r#"doc.set f.json key "trail\"#).unwrap_err();
        assert!(
            err.to_string()
                .contains("unexpected end of line after backslash"),
            "expected backslash error, got: {err}"
        );
    }

    #[test]
    fn tokenize_unterminated_quote_error() {
        let err = tokenize(r#"doc.set f.json key "no close"#).unwrap_err();
        assert!(
            err.to_string().contains("unterminated double quote"),
            "expected unterminated quote error, got: {err}"
        );
    }

    #[test]
    fn parse_line_unknown_op() {
        let err = parse_line("unknown.op foo bar", 1).unwrap_err();
        assert!(err.to_string().contains("unknown operation"));
    }

    // Batch intentionally does not support read, search, and patch.apply.
    // These are tx-only operations. The tests below document this as deliberate.

    #[test]
    fn parse_line_rejects_read() {
        let err = parse_line("read path.txt", 1).unwrap_err();
        assert!(err.to_string().contains("unknown operation"));
    }

    #[test]
    fn parse_line_rejects_search() {
        let err = parse_line("search path.txt hello", 1).unwrap_err();
        assert!(err.to_string().contains("unknown operation"));
    }

    #[test]
    fn parse_line_rejects_patch_apply() {
        let err = parse_line("patch.apply diff-text", 1).unwrap_err();
        assert!(err.to_string().contains("unknown operation"));
    }

    #[test]
    fn parse_line_too_few_args() {
        let err = parse_line("doc.set config.json", 1).unwrap_err();
        assert!(err.to_string().contains("requires exactly 3 arguments"));
    }

    #[test]
    fn parse_line_extra_args_rejected() {
        let err = parse_line(r#"file.delete old.txt extra"#, 1).unwrap_err();
        assert!(err.to_string().contains("requires exactly 1 arguments"));
    }

    #[test]
    fn parse_line_extra_args_rejected_all_operations() {
        // 2-arg operations (require exactly 2)
        let two_arg_ops = [
            r#"doc.delete f.json sel extra"#,
            r#"doc.merge f.json "{}" extra"#,
            r#"file.create f.txt content extra"#,
            r#"file.rename old.txt new.txt extra"#,
        ];
        for line in &two_arg_ops {
            let err = parse_line(line, 1).unwrap_err();
            assert!(
                err.to_string().contains("requires exactly 2 arguments"),
                "expected rejection for '{line}', got: {err}"
            );
        }

        // 3-arg operations (require exactly 3)
        let three_arg_ops = [
            r#"doc.set f.json sel "v" extra"#,
            r#"doc.ensure f.json sel "v" extra"#,
            r#"doc.append f.json sel "v" extra"#,
            r#"doc.prepend f.json sel "v" extra"#,
            r#"doc.update f.json sel "v" extra"#,
            r#"doc.move f.json from to extra"#,
            r#"doc.delete_where f.json sel "k=v" extra"#,
            r#"replace f.txt old new extra"#,
            r##"md.upsert_bullet f.md "# H" "- b" extra"##,
            r##"md.table_append f.md "# H" "| r |" extra"##,
            r##"md.replace_section f.md "# H" body extra"##,
            r##"md.insert_after_heading f.md "# H" text extra"##,
            r##"md.insert_before_heading f.md "# H" text extra"##,
        ];
        for line in &three_arg_ops {
            let err = parse_line(line, 1).unwrap_err();
            assert!(
                err.to_string().contains("requires exactly 3 arguments"),
                "expected rejection for '{line}', got: {err}"
            );
        }

        // 1-arg operations (require exactly 1)
        let one_arg_ops = [
            "md.dedupe_headings f.md extra",
            "md.lint_agents f.md extra",
            "tidy.fix f.txt extra",
        ];
        for line in &one_arg_ops {
            let err = parse_line(line, 1).unwrap_err();
            assert!(
                err.to_string().contains("requires exactly 1 arguments"),
                "expected rejection for '{line}', got: {err}"
            );
        }
    }

    #[test]
    fn tokenize_error_includes_line_number() {
        // Unterminated quote should include the line number from parse_line.
        let err = parse_line(r#"doc.set f.json key "unterminated"#, 7).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("line 7"),
            "expected line number in error: {msg}"
        );
        assert!(
            msg.contains("unterminated double quote"),
            "expected tokenize message in error: {msg}"
        );
    }

    #[test]
    fn parse_line_md_move_section_bad_keyword() {
        let err =
            parse_line("md.move_section README.md \"FAQ\" between \"License\"", 1).unwrap_err();
        assert!(err.to_string().contains("expected 'before' or 'after'"));
    }

    #[test]
    fn parse_line_md_move_section_wrong_arg_count() {
        let err = parse_line("md.move_section README.md", 1).unwrap_err();
        assert!(err.to_string().contains("requires 4 args"));
    }
}

mod security {
    use super::*;

    #[test]
    fn max_batch_operations_limit_is_enforced() {
        let dir = tempfile::TempDir::new().unwrap();
        // Build input with MAX+1 lines.
        let lines: String = (0..=MAX_BATCH_OPERATIONS)
            .map(|i| format!("doc.set f.json key{i} \"v\""))
            .collect::<Vec<_>>()
            .join("\n");
        let input_file = dir.path().join("ops.txt");
        std::fs::write(&input_file, &lines).unwrap();

        let args = BatchArgs {
            input: input_file.to_str().unwrap().to_string(),
            write: Default::default(),
        };
        let global = GlobalFlags::test_with_cwd(dir.path());
        let err = run(args, &global).unwrap_err();
        assert!(
            err.to_string().contains("too many operations"),
            "expected limit error: {err}"
        );
    }
}
