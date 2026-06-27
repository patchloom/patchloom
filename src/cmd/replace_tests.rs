use super::*;
use std::fs;
use tempfile::TempDir;

fn make_args(from: &str, to: &str, paths: Vec<String>) -> ReplaceArgs {
    ReplaceArgs {
        from: from.to_string(),
        to: Some(to.to_string()),
        insert_before: None,
        insert_after: None,
        paths,
        literal: true,
        regex: false,
        if_exists: false,
        multiline: false,
        nth: None,
        case_insensitive: false,
        word_boundary: false,
        whole_line: false,
        range: None,
        write: Default::default(),
    }
}

mod basic {
    use super::*;

    #[test]
    fn literal_replace_works() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\nhello again\n").unwrap();

        let args = make_args(
            "hello",
            "hi",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 2);
        assert_eq!(replacements[0].replaced, "hi world\nhi again\n");
    }

    #[test]
    fn regex_replace_works() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "foo123bar\nfoo456baz\n").unwrap();

        let args = ReplaceArgs {
            from: r"foo\d+".to_string(),
            to: Some("replaced".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: false,
            regex: true,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 2);
        assert_eq!(replacements[0].replaced, "replacedbar\nreplacedbaz\n");
    }

    #[test]
    fn regex_capture_groups_in_replacement() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "version = \"1.2.3\"\n").unwrap();

        let args = ReplaceArgs {
            from: r#"version = "(\d+)\.(\d+)\.(\d+)""#.to_string(),
            to: Some(r#"version = "$1.$2.99""#.to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: false,
            regex: true,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(replacements.len(), 1);
        assert_eq!(
            replacements[0].replaced, "version = \"1.2.99\"\n",
            "capture groups $1/$2 should work in replacement text"
        );
    }

    #[test]
    fn diff_mode_produces_unified_diff() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "old line\n").unwrap();

        let args = make_args(
            "old",
            "new",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(replacements.len(), 1);

        // Verify the replacement content produces a valid diff.
        let diff = crate::diff::unified_diff(
            &replacements[0].display_path,
            &replacements[0].original,
            &replacements[0].replaced,
        );
        let diff_str = crate::diff::format_diff_result_colored(
            &crate::diff::DiffResult { diffs: vec![diff] },
            false,
        );
        assert!(diff_str.contains("--- a/"));
        assert!(diff_str.contains("+++ b/"));
        assert!(diff_str.contains("-old line"));
        assert!(diff_str.contains("+new line"));
    }

    #[test]
    fn apply_mode_writes_replacement() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = make_args(
            "hello",
            "hi",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hi world\n");
    }

    #[test]
    fn multi_file_replace() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), "hello from a\n").unwrap();
        fs::write(dir.path().join("b.txt"), "hello from b\n").unwrap();

        let args = make_args(
            "hello",
            "hi",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 2);
        let total: usize = replacements.iter().map(|r| r.match_count).sum();
        assert_eq!(total, 2);
    }

    #[test]
    fn if_exists_still_replaces_when_found() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = ReplaceArgs {
            from: "hello".to_string(),
            to: Some("hi".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: true,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hi world\n");
    }

    #[test]
    fn write_policy_ensure_final_newline_applied() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world").unwrap();

        let args = make_args(
            "hello",
            "hi",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let mut global = GlobalFlags::test_default();
        global.apply = true;
        global.ensure_final_newline = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hi world\n");
    }

    #[test]
    fn multiline_regex_spans_newlines() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "start\nmiddle\nend\n").unwrap();

        let args = ReplaceArgs {
            from: r"start.*end".to_string(),
            to: Some("replaced".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: false,
            regex: true,
            if_exists: false,
            multiline: true,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 1);
        assert_eq!(replacements[0].replaced, "replaced\n");
    }

    #[test]
    fn check_mode_returns_changes_detected() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = make_args(
            "hello",
            "hi",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let mut global = GlobalFlags::test_default();
        global.check = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);

        // File must not be modified in check mode.
        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hello world\n");
    }

    #[test]
    fn whole_line_deletes_matching_lines() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.rs");
        fs::write(
            &file,
            "fn main() {\n    let _x = foo();\n    let y = bar();\n    let _z = baz();\n}\n",
        )
        .unwrap();

        let args = ReplaceArgs {
            from: "let _".to_string(),
            to: Some(String::new()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: true,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 2);
        assert_eq!(
            replacements[0].replaced,
            "fn main() {\n    let y = bar();\n}\n"
        );
    }

    #[test]
    fn whole_line_regex_deletes_matching_lines() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.rs");
        fs::write(
            &file,
            "use std::io;\nuse std::fmt;\nuse crate::foo;\nuse crate::bar;\n",
        )
        .unwrap();

        let args = ReplaceArgs {
            from: r"use crate::".to_string(),
            to: Some(String::new()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: false,
            regex: true,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: true,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 2);
        assert_eq!(replacements[0].replaced, "use std::io;\nuse std::fmt;\n");
    }

    #[test]
    fn whole_line_replaces_entire_line() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "alpha\nbeta match\ngamma\n").unwrap();

        let args = ReplaceArgs {
            from: "match".to_string(),
            to: Some("replaced line".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: true,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].replaced, "alpha\nreplaced line\ngamma\n");
    }

    #[test]
    fn whole_line_with_range_restricts_matches() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "aaa\nbbb\nccc\nbbb\neee\n").unwrap();

        let args = ReplaceArgs {
            from: "bbb".to_string(),
            to: Some(String::new()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: true,
            range: Some("1:3".to_string()),
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        // Only the bbb on line 2 should be deleted; the one on line 4 is outside range.
        assert_eq!(replacements[0].match_count, 1);
        assert_eq!(replacements[0].replaced, "aaa\nccc\nbbb\neee\n");
    }

    #[test]
    fn whole_line_nth_only_removes_nth_match() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "aaa\nbbb\nccc\nbbb\neee\n").unwrap();

        let args = ReplaceArgs {
            from: "bbb".to_string(),
            to: Some(String::new()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: Some(2),
            case_insensitive: false,
            word_boundary: false,
            whole_line: true,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 1);
        // Second occurrence of bbb (line 4) is deleted.
        assert_eq!(replacements[0].replaced, "aaa\nbbb\nccc\neee\n");
    }

    #[test]
    fn word_boundary_prevents_partial_match() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.rs");
        fs::write(
            &file,
            "struct SetupFile {}\nstruct BenchSetupFile {}\nlet x: SetupFile = todo!();\n",
        )
        .unwrap();

        let args = ReplaceArgs {
            from: "SetupFile".to_string(),
            to: Some("NewFile".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: true,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 2);
        assert_eq!(
            replacements[0].replaced,
            "struct NewFile {}\nstruct BenchSetupFile {}\nlet x: NewFile = todo!();\n",
            "word_boundary should rename standalone SetupFile but not inside BenchSetupFile"
        );
    }
}

mod edge_cases {
    use super::*;

    #[test]
    fn if_exists_returns_success_on_no_matches() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = ReplaceArgs {
            from: "zzz_no_match_zzz".to_string(),
            to: Some("replacement".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: true,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let code = run(args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn binary_files_are_skipped() {
        let dir = TempDir::new().unwrap();
        let bin_file = dir.path().join("data.bin");
        // Write a file with NUL bytes (binary content).
        fs::write(&bin_file, b"hello\x00world").unwrap();

        let args = make_args(
            "hello",
            "replaced",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();
        assert!(
            replacements.is_empty(),
            "binary files should be skipped, got {} matches",
            replacements.len()
        );
    }

    #[test]
    fn multiline_false_does_not_span_newlines() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "start\nmiddle\nend\n").unwrap();

        let args = ReplaceArgs {
            from: r"start.*end".to_string(),
            to: Some("replaced".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: false,
            regex: true,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert!(
            replacements.is_empty(),
            "without multiline, dot should not match newlines"
        );
    }

    #[test]
    fn identity_replacement_treated_as_no_match() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        // Replacing "hello" with "hello" should produce no change.
        let args = make_args(
            "hello",
            "hello",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();
        assert!(
            replacements.is_empty(),
            "identity replacement must be filtered out"
        );
    }

    #[test]
    fn identity_replacement_check_returns_no_matches() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = make_args(
            "hello",
            "hello",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let mut global = GlobalFlags::test_default();
        global.check = true;

        let code = run(args, &global).unwrap();
        assert_eq!(
            code,
            exit::NO_MATCHES,
            "--check with identity replacement must not report changes"
        );
    }

    #[test]
    fn word_boundary_with_metacharacters() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.rs");
        fs::write(&file, "type A = Option<SetupFile>;\n").unwrap();

        let args = ReplaceArgs {
            from: "SetupFile".to_string(),
            to: Some("NewFile".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: true,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(
            replacements[0].replaced, "type A = Option<NewFile>;\n",
            "word_boundary should match at angle bracket boundaries"
        );
    }

    #[test]
    fn word_boundary_does_not_match_in_string() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.rs");
        // In a string, SetupFile is still at word boundaries (quotes are non-word chars)
        // so word_boundary alone won't skip strings -- that needs AST (#647).
        // But it WILL prevent matching inside compound identifiers.
        fs::write(
            &file,
            "let name = \"SetupFile\";\nlet x: SetupFileConfig = todo!();\n",
        )
        .unwrap();

        let args = ReplaceArgs {
            from: "SetupFile".to_string(),
            to: Some("NewFile".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: true,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        // SetupFile in the string IS matched (word boundaries don't know about strings)
        // SetupFileConfig is NOT matched (no word boundary between SetupFile and Config)
        assert_eq!(
            replacements[0].replaced,
            "let name = \"NewFile\";\nlet x: SetupFileConfig = todo!();\n",
        );
    }
}

mod error_handling {
    use super::*;

    #[test]
    fn no_matches_returns_exit_3() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = make_args(
            "zzz_no_match_zzz",
            "replacement",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let code = run(args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
    }

    #[test]
    fn nth_zero_is_rejected() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = ReplaceArgs {
            from: "hello".to_string(),
            to: Some("hi".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: Some(0),
            case_insensitive: false,
            word_boundary: false,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let err = run(args, &GlobalFlags::test_default()).unwrap_err();
        assert!(err.to_string().contains("1-based"), "{err}");
    }

    #[test]
    fn range_requires_whole_line() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello\n").unwrap();

        let args = ReplaceArgs {
            from: "hello".to_string(),
            to: Some("hi".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: false,
            range: Some("1:5".to_string()),
            write: Default::default(),
        };
        let err = run(args, &GlobalFlags::test_default()).unwrap_err();
        assert!(
            err.to_string().contains("range requires whole_line"),
            "{err}"
        );
    }

    #[test]
    fn whole_line_and_multiline_conflict() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello\n").unwrap();

        let args = ReplaceArgs {
            from: "hello".to_string(),
            to: Some("hi".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: false,
            regex: true,
            if_exists: false,
            multiline: true,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: true,
            range: None,
            write: Default::default(),
        };
        let err = run(args, &GlobalFlags::test_default()).unwrap_err();
        assert!(
            err.to_string()
                .contains("whole_line and multiline cannot be combined"),
            "{err}"
        );
    }
}
