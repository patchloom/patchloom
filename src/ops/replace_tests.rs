// ── replace module tests ──────────────────────────────────────────
mod replace_tests {
    use crate::ops::replace::*;

    fn valid_params() -> ReplaceValidationParams<'static> {
        ReplaceValidationParams {
            pattern: "needle",
            has_to: true,
            has_insert_before: false,
            has_insert_after: false,
            nth: None,
            whole_line: false,
            multiline: false,
            has_range: false,
        }
    }

    mod basic {
        use super::*;

        #[test]
        fn validate_mode_valid_to_only() {
            validate_replace_mode(true, false, false).unwrap();
        }

        #[test]
        fn validate_mode_valid_insert_before_only() {
            validate_replace_mode(false, true, false).unwrap();
        }

        #[test]
        fn validate_mode_valid_insert_after_only() {
            validate_replace_mode(false, false, true).unwrap();
        }

        #[test]
        fn validate_args_valid_basic() {
            validate_replace_args(&valid_params()).unwrap();
        }

        #[test]
        fn validate_args_nth_one_ok() {
            let mut p = valid_params();
            p.nth = Some(1);
            validate_replace_args(&p).unwrap();
        }

        #[test]
        fn validate_args_range_with_whole_line_ok() {
            let mut p = valid_params();
            p.has_range = true;
            p.whole_line = true;
            validate_replace_args(&p).unwrap();
        }

        #[test]
        fn validate_args_display_messages() {
            // Verify Display impl produces expected human-readable messages.
            assert!(
                ReplaceValidationError::EmptyPattern
                    .to_string()
                    .contains("replace pattern must not be empty")
            );
            assert!(ReplaceValidationError::NthZero.to_string().contains("nth"));
            assert!(
                ReplaceValidationError::RangeRequiresWholeLine
                    .to_string()
                    .contains("range requires whole_line")
            );
            assert!(
                ReplaceValidationError::WholeLineMultilineConflict
                    .to_string()
                    .contains("whole_line and multiline")
            );
            let missing = ReplaceValidationError::Mode(ReplaceModeError::MissingMode).to_string();
            assert!(
                missing.contains("--new")
                    && missing.contains("--insert-before")
                    && missing.contains("--insert-after"),
                "CLI flags first (#1829): {missing}"
            );
            assert!(
                missing.contains("not positional"),
                "hint for replace OLD NEW path: {missing}"
            );
        }

        #[test]
        fn replacement_text_with_to() {
            let result =
                replacement_text("from", &Some("to".into()), &None, &None, false, false, "");
            assert_eq!(result, "to");
        }

        #[test]
        fn replacement_text_insert_before_literal() {
            let result = replacement_text(
                "original",
                &None,
                &Some("PREFIX\n".into()),
                &None,
                false,
                false,
                "original",
            );
            assert_eq!(result, "PREFIX\noriginal");
        }

        #[test]
        fn replacement_text_insert_after_literal() {
            let result = replacement_text(
                "original",
                &None,
                &None,
                &Some("\nSUFFIX".into()),
                false,
                false,
                "original",
            );
            assert_eq!(result, "original\nSUFFIX");
        }

        #[test]
        fn replacement_text_insert_before_regex_anchor() {
            let result = replacement_text(
                "ignored",
                &None,
                &Some("PREFIX\n".into()),
                &None,
                true,
                true,
                "ignored",
            );
            assert_eq!(result, "PREFIX\n${0}");
        }

        #[test]
        fn replacement_text_insert_after_regex_anchor() {
            let result = replacement_text(
                "ignored",
                &None,
                &None,
                &Some("\nSUFFIX".into()),
                true,
                true,
                "ignored",
            );
            assert_eq!(result, "${0}\nSUFFIX");
        }

        // Regression: dollar signs in replacement text must be preserved
        // when case_insensitive/word_boundary compiles an internal regex.
        #[test]
        fn replacement_text_escapes_dollars_for_internal_regex() {
            // use_match_anchor=true (internal regex), regex_mode=false (not user-requested)
            let result =
                replacement_text("cost", &Some("$100".into()), &None, &None, true, false, "");
            assert_eq!(result, "$$100");
        }

        #[test]
        fn replacement_text_preserves_dollars_for_user_regex() {
            // use_match_anchor=true, regex_mode=true (user explicitly requested regex)
            let result = replacement_text(
                "(c)ost",
                &Some("$1ost".into()),
                &None,
                &None,
                true,
                true,
                "",
            );
            assert_eq!(result, "$1ost");
        }

        #[test]
        fn normalize_line_insert_after_comment_after_brace() {
            let file = "fn f() {\n}\n";
            let out =
                normalize_line_insert(file, "fn f() {", "    // comment\n", InsertSide::After);
            assert_eq!(out, "\n    // comment\n");
        }

        #[test]
        fn normalize_line_insert_after_whole_line_bare_payload() {
            // avoid alphabeta when every match is a whole line
            let file = "alpha\n";
            let out = normalize_line_insert(file, "alpha", "beta", InsertSide::After);
            assert_eq!(out, "\nbeta");
        }

        #[test]
        fn normalize_line_insert_after_whole_line_crlf_bare_payload() {
            // CRLF after anchor must still count as a line boundary, and the
            // separator must be CRLF so the file does not get mixed LF (fixrealloop).
            let file = "alpha\r\n";
            assert!(
                anchor_is_whole_line(file, "alpha"),
                "CRLF-terminated line should be whole-line"
            );
            let out = normalize_line_insert(file, "alpha", "beta", InsertSide::After);
            assert_eq!(out, "\r\nbeta");
        }

        #[test]
        fn normalize_line_insert_before_whole_line_crlf() {
            let file = "A\r\nB\r\n";
            let out = normalize_line_insert(file, "B", "PRE", InsertSide::Before);
            assert_eq!(out, "PRE\r\n");
        }

        #[test]
        fn normalize_line_insert_after_whole_line_bare_cr() {
            let file = "alpha\rbeta\r";
            // "alpha" is alone before CR; "beta" alone before CR.
            assert!(anchor_is_whole_line(file, "alpha"));
            let out = normalize_line_insert(file, "alpha", "x", InsertSide::After);
            assert_eq!(out, "\rx");
        }

        #[test]
        fn preferred_line_ending_prefers_crlf() {
            assert_eq!(preferred_line_ending("a\r\nb\n"), "\r\n");
            assert_eq!(preferred_line_ending("a\rb\r"), "\r");
            assert_eq!(preferred_line_ending("a\nb\n"), "\n");
            assert_eq!(preferred_line_ending(""), "\n");
        }

        #[test]
        fn normalize_line_insert_after_midline_stays_byte_exact() {
            let file = "prefix foo suffix\n";
            let out = normalize_line_insert(file, "foo", "X", InsertSide::After);
            assert_eq!(out, "X");
        }

        #[test]
        fn normalize_line_insert_before_appends_newline() {
            let file = "fn f() {\n}\n";
            let out = normalize_line_insert(file, "fn f() {", "// header", InsertSide::Before);
            assert_eq!(out, "// header\n");
        }

        #[test]
        fn normalize_line_insert_already_has_newline_unchanged() {
            let file = "fn f() {\n}\n";
            let after = normalize_line_insert(file, "fn f() {", "\n// c\n", InsertSide::After);
            assert_eq!(after, "\n// c\n");
            let before = normalize_line_insert(file, "fn f() {", "// c\n", InsertSide::Before);
            assert_eq!(before, "// c\n");
        }

        #[test]
        fn replace_content_literal_all() {
            let (out, count) = replace_content("aXbXc", "X", "Y", None, None);
            assert_eq!(out, "aYbYc");
            assert_eq!(count, 2);
        }

        #[test]
        fn replace_content_literal_nth() {
            let (out, count) = replace_content("aXbXcX", "X", "Y", None, Some(2));
            assert_eq!(out, "aXbYcX");
            assert_eq!(count, 1);
        }

        #[test]
        fn replace_content_regex_all() {
            let re = regex::Regex::new(r"\d+").unwrap();
            let (out, count) = replace_content("a1b22c333", "unused", "N", Some(&re), None);
            assert_eq!(out, "aNbNcN");
            assert_eq!(count, 3);
        }

        #[test]
        fn replace_content_regex_nth() {
            let re = regex::Regex::new(r"\d+").unwrap();
            let (out, count) = replace_content("a1b22c333", "unused", "N", Some(&re), Some(2));
            assert_eq!(out, "a1bNc333");
            assert_eq!(count, 1);
        }

        #[test]
        fn replace_content_regex_capture_group() {
            let re = regex::Regex::new(r"(\w+)@(\w+)").unwrap();
            let (out, count) = replace_content("user@host", "unused", "$2=$1", Some(&re), None);
            assert_eq!(out, "host=user");
            assert_eq!(count, 1);
        }

        #[test]
        fn compile_regex_mode_returns_some() {
            let re = compile_replace_regex(r"\d+", true, false, false, false)
                .unwrap()
                .expect("regex mode should return Some");
            assert!(re.is_match("abc123"));
        }

        #[test]
        fn compile_regex_multiline_dot_matches_newline() {
            let re = compile_replace_regex("a.b", true, false, true, false)
                .unwrap()
                .unwrap();
            assert!(
                re.is_match("a\nb"),
                "multiline should make dot match newline"
            );
        }

        #[test]
        fn compile_literal_case_insensitive_returns_some() {
            let re = compile_replace_regex("hello", false, true, false, false)
                .unwrap()
                .expect("case-insensitive literal should return Some");
            assert!(re.is_match("HELLO"));
        }

        #[test]
        fn compile_plain_literal_returns_none() {
            let re = compile_replace_regex("hello", false, false, false, false).unwrap();
            assert!(re.is_none());
        }

        #[test]
        fn compile_word_boundary_literal() {
            let re = compile_replace_regex("SetupFile", false, false, false, true)
                .unwrap()
                .expect("word_boundary should return Some");
            assert!(re.is_match("SetupFile"), "should match standalone word");
            assert!(
                re.is_match("use crate::SetupFile;"),
                "should match at word boundary"
            );
            assert!(
                !re.is_match("BenchSetupFile"),
                "should NOT match inside a longer word"
            );
            assert!(
                !re.is_match("SetupFileConfig"),
                "should NOT match when followed by more word chars"
            );
        }

        #[test]
        fn compile_word_boundary_case_insensitive() {
            let re = compile_replace_regex("setupfile", false, true, false, true)
                .unwrap()
                .expect("word_boundary + case_insensitive should return Some");
            assert!(re.is_match("SetupFile"));
            assert!(!re.is_match("BenchSetupFile"));
        }

        #[test]
        fn regex_anchors_match_at_line_boundaries() {
            // ^$ should match empty lines within content, not just at the absolute
            // start/end of the string. This is the core fix for #1253.
            let re = compile_replace_regex("^$", true, false, false, false)
                .unwrap()
                .unwrap();
            let content = "hello\n\nworld\n";
            let (result, count) = replace_content(content, "^$", "BLANK", Some(&re), None);
            assert_eq!(count, 1, "should match the empty line");
            assert_eq!(&*result, "hello\nBLANK\nworld\n");
        }

        #[test]
        fn regex_caret_matches_line_start() {
            // ^ should match the start of every line, not just the start of the file.
            let re = compile_replace_regex("^#", true, false, false, false)
                .unwrap()
                .unwrap();
            let content = "# heading\ntext\n# another\n";
            let (result, count) = replace_content(content, "^#", "##", Some(&re), None);
            assert_eq!(count, 2, "should match both lines starting with #");
            assert_eq!(&*result, "## heading\ntext\n## another\n");
        }

        #[test]
        fn regex_dollar_matches_line_end() {
            // $ should match the end of every line, not just the end of the file.
            let re = compile_replace_regex(";$", true, false, false, false)
                .unwrap()
                .unwrap();
            let content = "let a = 1;\nlet b = 2\nlet c = 3;\n";
            let (result, count) = replace_content(content, ";$", "", Some(&re), None);
            assert_eq!(count, 2, "should match both lines ending with ;");
            assert_eq!(&*result, "let a = 1\nlet b = 2\nlet c = 3\n");
        }

        #[test]
        fn regex_empty_line_pattern_with_whitespace() {
            let re = compile_replace_regex(r"^\s*$", true, false, false, false)
                .unwrap()
                .unwrap();
            let content = "hello\n  \nworld\n";
            let (result, count) = replace_content(content, r"^\s*$", "BLANK", Some(&re), None);
            assert_eq!(count, 1, "should match the whitespace-only line");
            assert_eq!(&*result, "hello\nBLANK\nworld\n");
        }

        #[test]
        fn regex_anchors_nth_skips_phantom_eof() {
            // With multi_line(true), ^$ on "hello\n\nworld\n" produces matches:
            // 1. the empty line (real match)
            // 2. after the final \n (phantom zero-length EOF match)
            // nth=2 should NOT land on the phantom match; it should return no-match.
            let re = compile_replace_regex("^$", true, false, false, false)
                .unwrap()
                .unwrap();
            let content = "hello\n\nworld\n";
            let (result, count) = replace_content(content, "^$", "BLANK", Some(&re), Some(2));
            assert_eq!(count, 0, "nth=2 should not match the phantom EOF");
            assert_eq!(&*result, content, "content should be unchanged");
        }

        #[test]
        fn regex_anchors_nth_first_match() {
            // nth=1 should hit the real empty-line match.
            let re = compile_replace_regex("^$", true, false, false, false)
                .unwrap()
                .unwrap();
            let content = "hello\n\nworld\n";
            let (result, count) = replace_content(content, "^$", "BLANK", Some(&re), Some(1));
            assert_eq!(count, 1);
            assert_eq!(&*result, "hello\nBLANK\nworld\n");
        }

        #[test]
        fn regex_anchors_no_trailing_newline() {
            // Content without a trailing newline should still match ^$ on empty lines.
            let re = compile_replace_regex("^$", true, false, false, false)
                .unwrap()
                .unwrap();
            let content = "hello\n\nworld";
            let (result, count) = replace_content(content, "^$", "BLANK", Some(&re), None);
            assert_eq!(count, 1, "should match the empty line");
            assert_eq!(&*result, "hello\nBLANK\nworld");
        }

        #[test]
        fn regex_anchors_empty_file() {
            // An empty file: the only ^$ match is at position (0,0) which equals
            // content_len, so the trailing-match filter drops it. This is consistent
            // with search, which iterates lines and finds no lines in empty content.
            let re = compile_replace_regex("^$", true, false, false, false)
                .unwrap()
                .unwrap();
            let content = "";
            let (result, count) = replace_content(content, "^$", "BLANK", Some(&re), None);
            assert_eq!(count, 0, "empty file should produce no matches");
            assert_eq!(&*result, "");
        }

        #[test]
        fn whole_lines_delete_literal() {
            let content = "aaa\nbbb\nccc\nbbb\neee\n";
            let (result, count) = replace_whole_lines(content, "bbb", "", None, None, None);
            assert_eq!(count, 2);
            assert_eq!(&*result, "aaa\nccc\neee\n");
        }

        #[test]
        fn whole_lines_delete_regex() {
            let re = compile_replace_regex(r"let _\w+", true, false, false, false)
                .unwrap()
                .unwrap();
            let content = "fn main() {\n    let _x = foo();\n    let y = bar();\n}\n";
            let (result, count) = replace_whole_lines(content, "", "", Some(&re), None, None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "fn main() {\n    let y = bar();\n}\n");
        }

        #[test]
        fn whole_lines_replace_with_text() {
            let content = "alpha\nbeta\ngamma\n";
            let (result, count) =
                replace_whole_lines(content, "beta", "REPLACED", None, None, None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "alpha\nREPLACED\ngamma\n");
        }

        #[test]
        fn whole_lines_range_restriction() {
            let content = "aaa\nbbb\nccc\nbbb\neee\n";
            // Range 1:3 means lines 1-3 only. Second bbb is on line 4.
            let (result, count) =
                replace_whole_lines(content, "bbb", "", None, None, Some((1, Some(3))));
            assert_eq!(count, 1);
            assert_eq!(&*result, "aaa\nccc\nbbb\neee\n");
        }

        #[test]
        fn whole_lines_nth() {
            let content = "aaa\nbbb\nccc\nbbb\neee\n";
            let (result, count) = replace_whole_lines(content, "bbb", "", None, Some(2), None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "aaa\nbbb\nccc\neee\n");
        }

        #[test]
        fn whole_lines_regex_capture_groups() {
            let re = compile_replace_regex(r"version = (\d+)", true, false, false, false)
                .unwrap()
                .unwrap();
            let content = "name = foo\nversion = 3\nrelease = true\n";
            let (result, count) =
                replace_whole_lines(content, "", "version = ${1}00", Some(&re), None, None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "name = foo\nversion = 300\nrelease = true\n");
        }
    }

    mod line_endings {
        use super::*;

        #[test]
        fn whole_lines_cr_endings_literal() {
            let content = "aaa\rbbb\rccc\r";
            let (result, count) = replace_whole_lines(content, "bbb", "REPLACED", None, None, None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "aaa\rREPLACED\rccc\r");
        }

        #[test]
        fn whole_lines_cr_endings_delete() {
            let content = "aaa\rbbb\rccc\rbbb\reee\r";
            let (result, count) = replace_whole_lines(content, "bbb", "", None, None, None);
            assert_eq!(count, 2);
            assert_eq!(&*result, "aaa\rccc\reee\r");
        }

        #[test]
        fn whole_lines_cr_no_match_returns_borrowed() {
            let content = "aaa\rbbb\rccc\r";
            let (result, count) = replace_whole_lines(content, "zzz", "", None, None, None);
            assert_eq!(count, 0);
            assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
        }

        #[test]
        fn whole_lines_crlf_preserves_ending() {
            let content = "aaa\r\nbbb\r\nccc\r\n";
            let (result, count) = replace_whole_lines(content, "bbb", "REPLACED", None, None, None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "aaa\r\nREPLACED\r\nccc\r\n");
        }

        #[test]
        fn whole_lines_crlf_delete() {
            let content = "aaa\r\nbbb\r\nccc\r\n";
            let (result, count) = replace_whole_lines(content, "bbb", "", None, None, None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "aaa\r\nccc\r\n");
        }

        #[test]
        fn whole_lines_crlf_line_content_excludes_cr() {
            // Verify that line_content does NOT include the \r from CRLF,
            // so exact-match patterns work without trailing \r.
            let content = "hello\r\nworld\r\n";
            let (result, count) =
                replace_whole_lines(content, "hello", "MATCHED", None, None, None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "MATCHED\r\nworld\r\n");
        }

        #[test]
        fn whole_lines_crlf_regex_dollar_matches() {
            // Regex $ should match at end of line_content (before \r\n),
            // not fail because \r is included in line_content.
            let re = compile_replace_regex(r"hello$", true, false, false, false)
                .unwrap()
                .unwrap();
            let content = "hello\r\nworld\r\n";
            let (result, count) =
                replace_whole_lines(content, "", "MATCHED", Some(&re), None, None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "MATCHED\r\nworld\r\n");
        }

        #[test]
        fn whole_lines_cr_regex_capture_groups() {
            let re = compile_replace_regex(r"v(\d+)", true, false, false, false)
                .unwrap()
                .unwrap();
            let content = "name=foo\rv3\rrelease=true\r";
            let (result, count) =
                replace_whole_lines(content, "", "v${1}00", Some(&re), None, None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "name=foo\rv300\rrelease=true\r");
        }

        #[test]
        fn whole_lines_cr_last_line_no_ending() {
            let content = "aaa\rbbb\rccc";
            let (result, count) = replace_whole_lines(content, "ccc", "", None, None, None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "aaa\rbbb\r");
        }

        #[test]
        fn whole_lines_mixed_endings_each_preserved() {
            // Lines with different endings: LF, CRLF, CR, no-ending.
            let content = "aaa\nbbb\r\nccc\rddd";
            let (result, count) = replace_whole_lines(content, "aaa", "A", None, None, None);
            assert_eq!(count, 1);
            // Only the first line is replaced; its \n ending is preserved.
            assert_eq!(&*result, "A\nbbb\r\nccc\rddd");

            let (result2, count2) = replace_whole_lines(content, "bbb", "B", None, None, None);
            assert_eq!(count2, 1);
            // Second line replaced; its \r\n ending is preserved.
            assert_eq!(&*result2, "aaa\nB\r\nccc\rddd");

            let (result3, count3) = replace_whole_lines(content, "ccc", "C", None, None, None);
            assert_eq!(count3, 1);
            // Third line replaced; its \r ending is preserved.
            assert_eq!(&*result3, "aaa\nbbb\r\nC\rddd");
        }

        #[test]
        fn whole_lines_cr_range_restriction() {
            let content = "aaa\rbbb\rccc\rbbb\reee\r";
            // Range 1:3 means lines 1-3. Second bbb is on line 4.
            let (result, count) =
                replace_whole_lines(content, "bbb", "", None, None, Some((1, Some(3))));
            assert_eq!(count, 1);
            assert_eq!(&*result, "aaa\rccc\rbbb\reee\r");
        }

        #[test]
        fn whole_lines_cr_nth() {
            let content = "aaa\rbbb\rccc\rbbb\reee\r";
            let (result, count) = replace_whole_lines(content, "bbb", "", None, Some(2), None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "aaa\rbbb\rccc\reee\r");
        }
    }

    mod edge_cases {
        use super::*;

        #[test]
        fn replace_content_literal_no_match() {
            let (out, count) = replace_content("hello", "zzz", "y", None, None);
            assert_eq!(out, "hello");
            assert_eq!(count, 0);
            // Cow optimization: no-match must return Borrowed, not a cloned String.
            assert!(
                matches!(out, std::borrow::Cow::Borrowed(_)),
                "expected Cow::Borrowed for no-match, got Owned"
            );
        }

        #[test]
        fn replace_content_literal_nth_out_of_range() {
            let (out, count) = replace_content("aXb", "X", "Y", None, Some(5));
            assert_eq!(out, "aXb");
            assert_eq!(count, 0);
            assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
        }

        #[test]
        fn replace_content_regex_no_match_returns_borrowed() {
            let re = regex::Regex::new(r"\d+").unwrap();
            let (out, count) = replace_content("no digits here", "unused", "N", Some(&re), None);
            assert_eq!(out, "no digits here");
            assert_eq!(count, 0);
            assert!(
                matches!(out, std::borrow::Cow::Borrowed(_)),
                "regex no-match should return Cow::Borrowed"
            );
        }

        #[test]
        fn replace_content_regex_nth_no_match_returns_borrowed() {
            let re = regex::Regex::new(r"\d+").unwrap();
            let (out, count) = replace_content("no digits here", "unused", "N", Some(&re), Some(1));
            assert_eq!(out, "no digits here");
            assert_eq!(count, 0);
            assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
        }

        #[test]
        fn compile_word_boundary_escapes_metacharacters() {
            // Test that regex metacharacters in the pattern are properly escaped.
            // Use a pattern with dots and parens that would be regex-special.
            let re = compile_replace_regex("foo.bar()", false, false, false, true)
                .unwrap()
                .expect("word_boundary with metacharacters should return Some");
            // The escaped pattern should NOT match "fooXbar()" (dot is literal)
            assert!(
                !re.is_match("fooXbarX"),
                "dot should be literal, not wildcard"
            );
        }

        #[test]
        fn whole_lines_no_match_returns_borrowed() {
            let content = "aaa\nbbb\nccc\n";
            let (result, count) = replace_whole_lines(content, "zzz", "", None, None, None);
            assert_eq!(count, 0);
            assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
        }

        #[test]
        fn whole_lines_last_line_no_newline() {
            let content = "aaa\nbbb\nccc";
            let (result, count) = replace_whole_lines(content, "ccc", "", None, None, None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "aaa\nbbb\n");
        }
    }

    mod error_handling {
        use super::*;

        #[test]
        fn validate_mode_missing() {
            assert_eq!(
                validate_replace_mode(false, false, false),
                Err(ReplaceModeError::MissingMode)
            );
        }

        #[test]
        fn validate_mode_both_inserts() {
            assert_eq!(
                validate_replace_mode(false, true, true),
                Err(ReplaceModeError::BothInsertModes)
            );
        }

        #[test]
        fn validate_mode_to_with_insert() {
            assert_eq!(
                validate_replace_mode(true, true, false),
                Err(ReplaceModeError::ToWithInsert)
            );
            assert_eq!(
                validate_replace_mode(true, false, true),
                Err(ReplaceModeError::ToWithInsert)
            );
        }

        #[test]
        fn validate_args_empty_pattern() {
            let mut p = valid_params();
            p.pattern = "";
            assert_eq!(
                validate_replace_args(&p),
                Err(ReplaceValidationError::EmptyPattern)
            );
        }

        #[test]
        fn validate_args_nth_zero() {
            let mut p = valid_params();
            p.nth = Some(0);
            assert_eq!(
                validate_replace_args(&p),
                Err(ReplaceValidationError::NthZero)
            );
        }

        #[test]
        fn validate_args_range_requires_whole_line() {
            let mut p = valid_params();
            p.has_range = true;
            p.whole_line = false;
            assert_eq!(
                validate_replace_args(&p),
                Err(ReplaceValidationError::RangeRequiresWholeLine)
            );
        }

        #[test]
        fn validate_args_whole_line_multiline_conflict() {
            let mut p = valid_params();
            p.whole_line = true;
            p.multiline = true;
            assert_eq!(
                validate_replace_args(&p),
                Err(ReplaceValidationError::WholeLineMultilineConflict)
            );
        }

        #[test]
        fn validate_args_mode_error_propagated() {
            let p = ReplaceValidationParams {
                pattern: "needle",
                has_to: false,
                has_insert_before: false,
                has_insert_after: false,
                nth: None,
                whole_line: false,
                multiline: false,
                has_range: false,
            };
            assert_eq!(
                validate_replace_args(&p),
                Err(ReplaceValidationError::Mode(ReplaceModeError::MissingMode))
            );
        }

        #[test]
        fn compile_invalid_regex_returns_error() {
            let err = compile_replace_regex("(unclosed", true, false, false, false)
                .expect_err("expected error");
            assert!(err.to_string().contains("regex parse error"), "msg={err}");
            assert_eq!(
                crate::fallback::edit_error_kind(&err),
                Some(crate::fallback::EditErrorKind::InvalidInput)
            );
        }

        // Regression: --whole-line + --insert-before/after silently drops
        // non-matched line content because replace_whole_lines replaces the
        // entire line with just the insert+match text.
        #[test]
        fn validate_args_whole_line_insert_before_conflict() {
            let p = ReplaceValidationParams {
                pattern: "needle",
                has_to: false,
                has_insert_before: true,
                has_insert_after: false,
                nth: None,
                whole_line: true,
                multiline: false,
                has_range: false,
            };
            assert_eq!(
                validate_replace_args(&p),
                Err(ReplaceValidationError::WholeLineInsertConflict)
            );
        }

        #[test]
        fn validate_args_whole_line_insert_after_conflict() {
            let p = ReplaceValidationParams {
                pattern: "needle",
                has_to: false,
                has_insert_before: false,
                has_insert_after: true,
                nth: None,
                whole_line: true,
                multiline: false,
                has_range: false,
            };
            assert_eq!(
                validate_replace_args(&p),
                Err(ReplaceValidationError::WholeLineInsertConflict)
            );
        }
    }
}

// ── context_filtered_offset tests ─────────────────────────────────
mod context_filter_tests {
    use crate::ops::replace::context_filtered_offset;

    #[test]
    fn after_context_picks_correct_occurrence() {
        let content =
            "[database]\nhost = localhost\nport = 5432\n\n[cache]\nhost = localhost\nport = 6379\n";
        let offset =
            context_filtered_offset(content, "host = localhost", None, Some("port = 5432"));
        // Should pick the first occurrence (under [database]).
        assert_eq!(offset, Some(11)); // "[database]\n" is 11 bytes
    }

    /// Short anchors inside longer previous lines must score (JW alone is <0.8).
    #[test]
    fn before_context_short_anchor_inside_long_line() {
        let content = "prefix alpha foo more\nother foo\n";
        let offset = context_filtered_offset(content, "foo", Some("alpha"), None);
        assert_eq!(
            offset,
            Some(content.find("foo").unwrap()),
            "should pick the foo after alpha on the same/prior fragment"
        );
    }

    /// Same-line prefix: `--before-context alpha` on `alpha foo` / `beta foo`.
    #[test]
    fn before_context_same_line_prefix() {
        let content = "alpha foo\nbeta foo\n";
        let offset = context_filtered_offset(content, "foo", Some("alpha"), None);
        assert_eq!(offset, Some("alpha ".len()));
    }

    /// Same-line suffix for after-context.
    #[test]
    fn after_context_same_line_suffix() {
        let content = "foo alpha\nfoo beta\n";
        let offset = context_filtered_offset(content, "foo", None, Some("alpha"));
        assert_eq!(offset, Some(0));
    }

    #[test]
    fn before_context_picks_correct_occurrence() {
        let content =
            "[database]\nhost = localhost\nport = 5432\n\n[cache]\nhost = localhost\nport = 6379\n";
        let offset = context_filtered_offset(content, "host = localhost", Some("[cache]"), None);
        // Should pick the second occurrence (under [cache]).
        let expected = content.find("[cache]\n").unwrap() + "[cache]\n".len();
        assert_eq!(offset, Some(expected));
    }

    #[test]
    fn no_context_returns_none() {
        let content = "a\na\n";
        assert_eq!(context_filtered_offset(content, "a", None, None), None);
    }

    #[test]
    fn single_match_returns_none() {
        let content = "unique line\n";
        assert_eq!(
            context_filtered_offset(content, "unique line", None, Some("x")),
            None
        );
    }

    #[test]
    fn all_scores_zero_returns_none() {
        // When context doesn't match any occurrence (all Jaro-Winkler scores < 0.8),
        // the function should return None instead of picking an arbitrary match.
        let content = "x = 1\nval = hello\ny = 2\nval = hello\nz = 3\n";
        assert_eq!(
            context_filtered_offset(
                content,
                "val = hello",
                Some("completely unrelated context"),
                Some("nothing matches here either")
            ),
            None
        );
    }

    #[test]
    fn multiline_before_context_uses_nearest_line() {
        // Multi-line before_context: up to 3 lines are compared (nearest to match first).
        // Here, only the last context line matches, which is enough to disambiguate.
        let content =
            "[database]\nhost = localhost\nport = 5432\n\n[cache]\nhost = localhost\nport = 6379\n";
        let offset = context_filtered_offset(
            content,
            "host = localhost",
            Some("[cache]\nextra line\n[cache]"),
            None,
        );
        // The last line of before_context is "[cache]", which matches the second occurrence.
        let expected = content.find("[cache]\n").unwrap() + "[cache]\n".len();
        assert_eq!(offset, Some(expected));
    }

    #[test]
    fn multiline_after_context_uses_nearest_line() {
        // Multi-line after_context: the first line is compared (nearest to match).
        let content =
            "[database]\nhost = localhost\nport = 5432\n\n[cache]\nhost = localhost\nport = 6379\n";
        let offset = context_filtered_offset(
            content,
            "host = localhost",
            None,
            Some("port = 6379\nsome extra stuff"),
        );
        // The first line of after_context is "port = 6379", which matches the second occurrence.
        let expected = content.find("[cache]\n").unwrap() + "[cache]\n".len();
        assert_eq!(offset, Some(expected));
    }

    #[test]
    fn multiline_context_disambiguates_similar_sections() {
        // Both sections have "name = ..." before "host = localhost", so single-line
        // context would not distinguish them. Multi-line context uses the section
        // heading two lines above to pick the right one.
        let content = "\
[database]
name = mydb
host = localhost
port = 5432

[cache]
name = mycache
host = localhost
port = 6379
";
        let offset = context_filtered_offset(
            content,
            "host = localhost",
            Some("[cache]\nname = mycache"),
            None,
        );
        let expected =
            content.find("[cache]\n").unwrap() + "[cache]\n".len() + "name = mycache\n".len();
        assert_eq!(offset, Some(expected));
    }

    #[test]
    fn multiline_after_context_two_lines() {
        // Verify that two lines of after_context are both used for scoring.
        let content = "\
[web]
host = localhost
port = 80
ssl = false

[api]
host = localhost
port = 443
ssl = true
";
        let offset = context_filtered_offset(
            content,
            "host = localhost",
            None,
            Some("port = 443\nssl = true"),
        );
        let expected = content.find("[api]\n").unwrap() + "[api]\n".len();
        assert_eq!(offset, Some(expected));
    }
}

mod nth_count_tests {
    use crate::ops::replace::{
        count_content_matches, count_nth_candidates, count_whole_line_matches,
    };

    #[test]
    fn count_nth_candidates_whole_line_vs_substring() {
        let content = "a a\na\n";
        assert_eq!(count_content_matches(content, "a", None), 3);
        assert_eq!(count_whole_line_matches(content, "a", None, None), 2);
        assert_eq!(count_nth_candidates(content, "a", None, false, None), 3);
        assert_eq!(count_nth_candidates(content, "a", None, true, None), 2);
        // Range 2:4 on "a\nb\na\na\nc\n" has two whole-line hits (lines 3-4).
        let ranged = "a\nb\na\na\nc\n";
        assert_eq!(
            count_nth_candidates(ranged, "a", None, true, Some((2, Some(4)))),
            2
        );
    }
}
