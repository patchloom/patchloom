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
                    .contains("search pattern must not be empty")
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
            assert!(
                ReplaceValidationError::Mode(ReplaceModeError::MissingMode)
                    .to_string()
                    .contains("'to', 'insert_before', or 'insert_after'")
            );
        }

        #[test]
        fn replacement_text_with_to() {
            let result = replacement_text("from", &Some("to".into()), &None, &None, false, false);
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
            );
            assert_eq!(result, "${0}\nSUFFIX");
        }

        // Regression: dollar signs in replacement text must be preserved
        // when case_insensitive/word_boundary compiles an internal regex.
        #[test]
        fn replacement_text_escapes_dollars_for_internal_regex() {
            // use_match_anchor=true (internal regex), regex_mode=false (not user-requested)
            let result = replacement_text("cost", &Some("$100".into()), &None, &None, true, false);
            assert_eq!(result, "$$100");
        }

        #[test]
        fn replacement_text_preserves_dollars_for_user_regex() {
            // use_match_anchor=true, regex_mode=true (user explicitly requested regex)
            let result =
                replacement_text("(c)ost", &Some("$1ost".into()), &None, &None, true, true);
            assert_eq!(result, "$1ost");
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
        fn replace_content_empty_from_returns_borrowed() {
            let (out, count) = replace_content("hello", "", "y", None, None);
            assert_eq!(out, "hello");
            assert_eq!(count, 0);
            assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
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
            let result = compile_replace_regex("(unclosed", true, false, false, false);
            assert!(result.is_err());
        }
    }
}
