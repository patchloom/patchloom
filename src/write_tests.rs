use super::*;
#[cfg(feature = "cli")]
use crate::cli::global::GlobalFlags;
use std::fs;

#[cfg(feature = "cli")]
fn test_global_flags() -> GlobalFlags {
    GlobalFlags::test_default()
}

mod detect_eol_tests {
    use super::*;

    #[test]
    fn detect_eol_lf_only() {
        assert_eq!(detect_eol("line1\nline2\nline3\n"), "\n");
    }

    #[test]
    fn detect_eol_crlf_only() {
        assert_eq!(detect_eol("line1\r\nline2\r\nline3\r\n"), "\r\n");
    }

    #[test]
    fn detect_eol_mixed_crlf_dominant() {
        assert_eq!(detect_eol("line1\r\nline2\nline3\r\n"), "\r\n");
    }

    #[test]
    fn detect_eol_mixed_lf_dominant() {
        assert_eq!(detect_eol("line1\nline2\r\nline3\n"), "\n");
    }

    #[test]
    fn detect_eol_empty_string() {
        assert_eq!(detect_eol(""), "\n");
    }

    #[test]
    fn detect_eol_no_newlines() {
        assert_eq!(detect_eol("no newlines here"), "\n");
    }
}

mod basic {
    use super::*;

    #[test]
    fn ensure_final_newline_adds_when_missing() {
        assert_eq!(ensure_final_newline("hello", EolMode::Keep), "hello\n");
    }

    #[test]
    fn parse_eol_mode_cr() {
        assert!(matches!(parse_eol_mode("cr").unwrap(), EolMode::Cr));
    }

    #[test]
    fn trim_trailing_whitespace_removes_spaces() {
        let result = trim_trailing_whitespace("hello   \nworld\t\n");
        assert_eq!(result, "hello\nworld\n");
        assert!(
            matches!(result, std::borrow::Cow::Owned(_)),
            "trimmed content should be Cow::Owned"
        );
    }

    #[test]
    fn trim_trailing_whitespace_clean_returns_borrowed() {
        let result = trim_trailing_whitespace("hello\nworld\n");
        assert_eq!(result, "hello\nworld\n");
        assert!(
            matches!(result, std::borrow::Cow::Borrowed(_)),
            "clean content should return Cow::Borrowed, not allocate"
        );
    }

    #[test]
    fn noop_policy_returns_borrowed() {
        let policy = WritePolicy::default();
        let input = "hello\nworld\n";
        let result = apply_policy(input, &policy);
        assert!(
            matches!(result, std::borrow::Cow::Borrowed(_)),
            "no-op policy should return Cow::Borrowed"
        );
        assert_eq!(&*result, input);
    }

    #[test]
    fn apply_policy_chains_all() {
        let policy = WritePolicy {
            trim_trailing_whitespace: true,
            normalize_eol: EolMode::Lf,
            ensure_final_newline: true,
            ..Default::default()
        };
        // Trailing whitespace, CRLF endings, no final newline.
        let input = "hello  \r\nworld\t\r\n";
        let result = apply_policy(input, &policy);
        // After trim: "hello\r\nworld\r\n"
        // After LF:   "hello\nworld\n"
        // After final newline: already ends with \n → unchanged.
        assert_eq!(result, "hello\nworld\n");
    }

    #[test]
    fn atomic_write_writes_correct_content() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("output.txt");

        let policy = WritePolicy {
            ensure_final_newline: true,
            normalize_eol: EolMode::Lf,
            trim_trailing_whitespace: true,
            ..Default::default()
        };

        atomic_write(&target, "foo  \r\nbar", &policy).unwrap();

        let got = fs::read_to_string(&target).unwrap();
        assert_eq!(got, "foo\nbar\n");
    }

    #[test]
    #[cfg(feature = "cli")]
    fn policy_from_flags_explicit_flags_win() {
        let dir = tempfile::tempdir().unwrap();
        let ec_path = dir.path().join(".editorconfig");
        fs::write(
            &ec_path,
            "root = true\n\n[*]\ninsert_final_newline = false\nend_of_line = crlf\ntrim_trailing_whitespace = false\n",
        )
        .unwrap();

        let file = dir.path().join("test.txt");
        fs::write(&file, "content\n").unwrap();

        let mut global = test_global_flags();
        global.respect_editorconfig = true;
        global.ensure_final_newline = true;
        global.normalize_eol = Some(EolMode::Lf);
        global.trim_trailing_whitespace = true;

        let policy = policy_from_flags(&global, Some(&file));
        // Explicit flags should win over EditorConfig values.
        assert!(policy.ensure_final_newline);
        assert!(matches!(policy.normalize_eol, EolMode::Lf));
        assert!(policy.trim_trailing_whitespace);
    }

    #[test]
    #[cfg(feature = "cli")]
    fn policy_from_flags_editorconfig_provides_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let ec_path = dir.path().join(".editorconfig");
        fs::write(
            &ec_path,
            "root = true\n\n[*]\ninsert_final_newline = true\nend_of_line = lf\ntrim_trailing_whitespace = true\n",
        )
        .unwrap();

        let file = dir.path().join("test.txt");
        fs::write(&file, "content\n").unwrap();

        let mut global = test_global_flags();
        global.respect_editorconfig = true;

        let policy = policy_from_flags(&global, Some(&file));
        assert!(policy.ensure_final_newline);
        assert!(matches!(policy.normalize_eol, EolMode::Lf));
        assert!(policy.trim_trailing_whitespace);
    }

    #[test]
    fn noop_policy_detected() {
        assert!(WritePolicy::default().is_noop());
    }

    #[test]
    fn non_noop_policy_detected() {
        let p = WritePolicy {
            ensure_final_newline: true,
            ..Default::default()
        };
        assert!(!p.is_noop());

        let p2 = WritePolicy {
            normalize_eol: EolMode::Lf,
            ..Default::default()
        };
        assert!(!p2.is_noop());

        let p3 = WritePolicy {
            trim_trailing_whitespace: true,
            ..Default::default()
        };
        assert!(!p3.is_noop());
    }

    #[test]
    fn atomic_create_new_writes_content() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("new.txt");

        let policy = WritePolicy {
            ensure_final_newline: true,
            normalize_eol: EolMode::Lf,
            trim_trailing_whitespace: false,
            ..Default::default()
        };

        atomic_create_new(&target, "hello", &policy).unwrap();
        let got = fs::read_to_string(&target).unwrap();
        assert_eq!(got, "hello\n");
    }

    #[test]
    #[cfg(feature = "cli")]
    fn policy_from_flags_no_editorconfig_uses_defaults() {
        let global = test_global_flags();

        let policy = policy_from_flags(&global, None);
        assert!(!policy.ensure_final_newline);
        assert!(matches!(policy.normalize_eol, EolMode::Keep));
        assert!(!policy.trim_trailing_whitespace);
    }

    #[test]
    fn collapse_blanks_reduces_consecutive_blanks() {
        let input = "line1\n\n\n\nline2\n\n\nline3\n";
        let result = collapse_blanks(input);
        assert_eq!(result, "line1\n\nline2\n\nline3\n");
    }

    #[test]
    fn collapse_blanks_no_change_returns_borrowed() {
        let input = "line1\n\nline2\nline3\n";
        let result = collapse_blanks(input);
        assert_eq!(result, input);
        assert!(
            matches!(result, std::borrow::Cow::Borrowed(_)),
            "no-change should return Cow::Borrowed"
        );
    }

    #[test]
    fn apply_policy_collapse_blanks() {
        let policy = WritePolicy {
            collapse_blanks: true,
            ..Default::default()
        };
        let input = "a\n\n\nb\n";
        let result = apply_policy(input, &policy);
        assert_eq!(result, "a\n\nb\n");
    }

    #[test]
    fn write_policy_override_full() {
        let mut policy = WritePolicy::default();
        let ov = WritePolicyOverride {
            ensure_final_newline: Some(true),
            normalize_eol: Some("lf".to_string()),
            trim_trailing_whitespace: Some(true),
            collapse_blanks: Some(true),
            respect_editorconfig: Some(true),
        };
        policy.apply_override(&ov).unwrap();
        assert!(policy.ensure_final_newline);
        assert!(matches!(policy.normalize_eol, EolMode::Lf));
        assert!(policy.trim_trailing_whitespace);
        assert!(policy.collapse_blanks);
    }

    #[test]
    fn write_policy_override_partial() {
        let mut policy = WritePolicy::default();
        let ov = WritePolicyOverride {
            ensure_final_newline: Some(true),
            ..Default::default()
        };
        policy.apply_override(&ov).unwrap();
        assert!(policy.ensure_final_newline);
        // Other fields stay at defaults.
        assert!(matches!(policy.normalize_eol, EolMode::Keep));
        assert!(!policy.trim_trailing_whitespace);
        assert!(!policy.collapse_blanks);
    }

    #[test]
    fn write_policy_override_lf() {
        let mut policy = WritePolicy::default();
        let ov = WritePolicyOverride {
            normalize_eol: Some("lf".to_string()),
            ..Default::default()
        };
        policy.apply_override(&ov).unwrap();
        assert!(matches!(policy.normalize_eol, EolMode::Lf));
    }

    #[test]
    fn write_policy_override_crlf() {
        let mut policy = WritePolicy::default();
        let ov = WritePolicyOverride {
            normalize_eol: Some("crlf".to_string()),
            ..Default::default()
        };
        policy.apply_override(&ov).unwrap();
        assert!(matches!(policy.normalize_eol, EolMode::Crlf));
    }

    #[test]
    fn write_policy_override_cr() {
        let mut policy = WritePolicy::default();
        let ov = WritePolicyOverride {
            normalize_eol: Some("cr".to_string()),
            ..Default::default()
        };
        policy.apply_override(&ov).unwrap();
        assert!(matches!(policy.normalize_eol, EolMode::Cr));
    }
}

mod line_endings {
    use super::*;

    #[test]
    fn ensure_final_newline_cr_mode_appends_cr() {
        assert_eq!(ensure_final_newline("hello\r", EolMode::Cr), "hello\r");
        assert_eq!(ensure_final_newline("hello", EolMode::Cr), "hello\r");
    }

    #[test]
    fn ensure_final_newline_cr_mode_does_not_append_lf() {
        // Content ending with \r should NOT get a \n appended
        let result = ensure_final_newline("line1\rline2\r", EolMode::Cr);
        assert!(result.ends_with('\r'));
        assert!(!result.ends_with('\n'));
    }

    #[test]
    fn ensure_final_newline_crlf_mode_appends_crlf() {
        assert_eq!(
            ensure_final_newline("hello\r\n", EolMode::Crlf),
            "hello\r\n"
        );
        assert_eq!(ensure_final_newline("hello", EolMode::Crlf), "hello\r\n");
    }

    #[test]
    fn ensure_final_newline_crlf_mode_bare_lf_gets_crlf() {
        // Content ending with bare \n should get \r\n appended (not kept as-is)
        assert_eq!(
            ensure_final_newline("hello\n", EolMode::Crlf),
            "hello\n\r\n"
        );
    }

    #[test]
    fn ensure_final_newline_lf_mode_unchanged() {
        assert_eq!(ensure_final_newline("hello", EolMode::Lf), "hello\n");
        assert_eq!(ensure_final_newline("hello\n", EolMode::Lf), "hello\n");
    }

    #[test]
    fn normalize_eol_lf_converts_crlf() {
        assert_eq!(normalize_eol("a\r\nb\r\n", EolMode::Lf), "a\nb\n");
    }

    #[test]
    fn normalize_eol_crlf_converts_lf() {
        assert_eq!(normalize_eol("a\nb\n", EolMode::Crlf), "a\r\nb\r\n");
    }

    #[test]
    fn normalize_eol_crlf_bare_lf_at_position_zero() {
        // Exercises the `i == 0` branch in the memchr single-pass scan.
        assert_eq!(normalize_eol("\na\n", EolMode::Crlf), "\r\na\r\n");
    }

    #[test]
    fn normalize_eol_crlf_mixed_content() {
        // Some lines already CRLF, some bare LF — only bare LFs get \r.
        assert_eq!(
            normalize_eol("a\r\nb\nc\r\n", EolMode::Crlf),
            "a\r\nb\r\nc\r\n"
        );
    }

    #[test]
    fn normalize_eol_crlf_already_correct_returns_borrowed() {
        use std::borrow::Cow;
        let content = "a\r\nb\r\n";
        let result = normalize_eol(content, EolMode::Crlf);
        assert!(
            matches!(result, Cow::Borrowed(_)),
            "all-CRLF content should return Cow::Borrowed"
        );
    }

    #[test]
    fn normalize_eol_keep_unchanged() {
        let content = "a\r\nb\nc\n";
        assert_eq!(normalize_eol(content, EolMode::Keep), content);
    }

    #[test]
    fn normalize_eol_cr_converts_lf() {
        assert_eq!(normalize_eol("a\nb\n", EolMode::Cr), "a\rb\r");
    }

    #[test]
    fn normalize_eol_cr_converts_crlf() {
        assert_eq!(normalize_eol("a\r\nb\r\n", EolMode::Cr), "a\rb\r");
    }

    #[test]
    fn normalize_eol_cr_mixed_input() {
        assert_eq!(normalize_eol("a\r\nb\nc\r\n", EolMode::Cr), "a\rb\rc\r");
    }

    #[test]
    fn normalize_eol_cr_already_correct_returns_borrowed() {
        use std::borrow::Cow;
        let content = "a\rb\r";
        let result = normalize_eol(content, EolMode::Cr);
        assert!(
            matches!(result, Cow::Borrowed(_)),
            "all-CR content should return Cow::Borrowed"
        );
    }

    #[test]
    fn normalize_eol_lf_also_strips_bare_cr() {
        // LF mode should convert both \r\n and bare \r to \n.
        assert_eq!(normalize_eol("a\rb\r\nc\n", EolMode::Lf), "a\nb\nc\n");
    }

    #[test]
    fn normalize_eol_crlf_converts_bare_cr() {
        // Bare \r (classic Mac) should become \r\n.
        assert_eq!(normalize_eol("a\rb\r", EolMode::Crlf), "a\r\nb\r\n");
    }

    #[test]
    fn normalize_eol_crlf_mixed_with_bare_cr() {
        // Mix of bare \r, \r\n, and bare \n should all become \r\n.
        assert_eq!(
            normalize_eol("a\rb\r\nc\n", EolMode::Crlf),
            "a\r\nb\r\nc\r\n"
        );
    }

    #[test]
    #[cfg(feature = "cli")]
    fn policy_from_flags_editorconfig_cr() {
        let dir = tempfile::tempdir().unwrap();
        let ec_path = dir.path().join(".editorconfig");
        fs::write(&ec_path, "root = true\n\n[*]\nend_of_line = cr\n").unwrap();

        let file = dir.path().join("test.txt");
        fs::write(&file, "content\n").unwrap();

        let mut global = test_global_flags();
        global.respect_editorconfig = true;

        let policy = policy_from_flags(&global, Some(&file));
        assert!(
            matches!(policy.normalize_eol, EolMode::Cr),
            "end_of_line = cr should map to EolMode::Cr"
        );
    }

    #[test]
    fn trim_trailing_whitespace_crlf_endings() {
        let result = trim_trailing_whitespace("hello  \r\nworld\t\r\n");
        assert_eq!(result, "hello\r\nworld\r\n");
    }

    #[test]
    fn trim_trailing_whitespace_cr_endings() {
        let result = trim_trailing_whitespace("hello   \rworld\t\r");
        assert_eq!(result, "hello\rworld\r");
        assert!(matches!(result, std::borrow::Cow::Owned(_)));
    }

    #[test]
    fn trim_trailing_whitespace_cr_clean_returns_borrowed() {
        let result = trim_trailing_whitespace("hello\rworld\r");
        assert_eq!(result, "hello\rworld\r");
        assert!(
            matches!(result, std::borrow::Cow::Borrowed(_)),
            "clean CR content should return Cow::Borrowed"
        );
    }

    #[test]
    fn trim_trailing_whitespace_cr_mixed_whitespace() {
        // Tabs and spaces before CR endings.
        let result = trim_trailing_whitespace("a \t\rb  \r");
        assert_eq!(result, "a\rb\r");
    }

    #[test]
    fn trim_trailing_whitespace_cr_no_trailing_newline() {
        // Last line with trailing whitespace, no line ending.
        let result = trim_trailing_whitespace("hello\rworld  ");
        assert_eq!(result, "hello\rworld");
    }

    #[test]
    fn collapse_blanks_cr_endings() {
        let input = "line1\r\r\r\rline2\r\r\rline3\r";
        let result = collapse_blanks(input);
        assert_eq!(result, "line1\r\rline2\r\rline3\r");
    }

    #[test]
    fn collapse_blanks_cr_no_change_returns_borrowed() {
        let input = "line1\r\rline2\rline3\r";
        let result = collapse_blanks(input);
        assert_eq!(result, input);
        assert!(
            matches!(result, std::borrow::Cow::Borrowed(_)),
            "no-change CR content should return Cow::Borrowed"
        );
    }

    #[test]
    fn collapse_blanks_crlf_endings() {
        let input = "line1\r\n\r\n\r\n\r\nline2\r\n";
        let result = collapse_blanks(input);
        assert_eq!(result, "line1\r\n\r\nline2\r\n");
    }

    #[test]
    fn collapse_blanks_cr_whitespace_only_lines_are_blank() {
        let input = "line1\r  \r\t\r\rline2\r";
        let result = collapse_blanks(input);
        // "  " and "\t" and "" are all blank; three consecutive blanks become one.
        assert_eq!(result, "line1\r  \rline2\r");
    }

    #[test]
    fn apply_policy_cr_mode_final_newline_is_cr() {
        let policy = WritePolicy {
            ensure_final_newline: true,
            normalize_eol: EolMode::Cr,
            ..Default::default()
        };
        // Content without a trailing newline: should get \r appended.
        let result = apply_policy("hello", &policy);
        assert_eq!(result, "hello\r");
        assert!(!result.ends_with('\n'), "CR mode must not append \\n");
    }

    #[test]
    fn apply_policy_crlf_mode_final_newline_is_crlf() {
        let policy = WritePolicy {
            ensure_final_newline: true,
            normalize_eol: EolMode::Crlf,
            ..Default::default()
        };
        let result = apply_policy("hello", &policy);
        assert_eq!(result, "hello\r\n");
    }

    #[test]
    fn apply_policy_cr_mode_multiline_final_newline() {
        let policy = WritePolicy {
            ensure_final_newline: true,
            normalize_eol: EolMode::Cr,
            ..Default::default()
        };
        // Input has LF line endings; normalize converts to CR, then final \r appended.
        let result = apply_policy("a\nb", &policy);
        assert_eq!(result, "a\rb\r");
    }

    #[test]
    fn apply_policy_cr_mode_collapse_blanks() {
        let policy = WritePolicy {
            collapse_blanks: true,
            normalize_eol: EolMode::Cr,
            ..Default::default()
        };
        // Input with LF endings; after normalize_eol they become CR.
        // Then collapse_blanks must still detect and collapse consecutive blank lines.
        let input = "a\n\n\nb\n";
        let result = apply_policy(input, &policy);
        assert_eq!(result, "a\r\rb\r");
    }

    #[test]
    fn apply_policy_cr_mode_trim_and_collapse() {
        let policy = WritePolicy {
            trim_trailing_whitespace: true,
            collapse_blanks: true,
            normalize_eol: EolMode::Cr,
            ensure_final_newline: true,
        };
        // Full pipeline: trim trailing ws, normalize to CR, collapse blanks, ensure final \r.
        let input = "hello  \n\n\nworld\t\n";
        let result = apply_policy(input, &policy);
        // After trim: "hello\n\n\nworld\n"
        // After CR normalize: "hello\r\r\rworld\r"
        // After collapse: "hello\r\rworld\r"
        // After final newline: already ends with \r
        assert_eq!(result, "hello\r\rworld\r");
    }
}

mod edge_cases {
    use super::*;

    #[test]
    fn ensure_final_newline_empty_stays_empty() {
        assert_eq!(ensure_final_newline("", EolMode::Keep), "");
    }

    #[test]
    fn ensure_final_newline_no_double_add() {
        assert_eq!(ensure_final_newline("hello\n", EolMode::Keep), "hello\n");
    }

    #[test]
    fn trim_trailing_whitespace_eof_without_newline() {
        let result = trim_trailing_whitespace("hello  ");
        assert_eq!(result, "hello");
    }

    #[test]
    fn collapse_blanks_whitespace_only_lines_are_blank() {
        let input = "line1\n  \n\t\n\nline2\n";
        let result = collapse_blanks(input);
        // "  " and "\t" and "" are all blank; three consecutive blanks become one.
        assert_eq!(result, "line1\n  \nline2\n");
    }

    #[test]
    fn collapse_blanks_no_blanks() {
        let input = "line1\nline2\nline3\n";
        let result = collapse_blanks(input);
        assert_eq!(result, input);
    }

    #[test]
    fn collapse_blanks_trailing_blank_no_newline() {
        // Two consecutive blank lines where the last has no trailing newline.
        // The fast-scan must detect this to trigger collapsing.
        // The first blank line is preserved (with its newline); the second is dropped.
        let input = "a\n \n ";
        let result = collapse_blanks(input);
        assert_eq!(result, "a\n \n");
    }
}

mod dedent_indent {
    use super::*;

    #[test]
    fn dedent_auto_removes_minimum_indent() {
        let input = "    line1\n        line2\n    line3\n";
        let result = dedent_content(input, "auto", None);
        assert_eq!(result, "line1\n    line2\nline3\n");
    }

    #[test]
    fn dedent_auto_skips_blank_lines() {
        let input = "    line1\n\n    line2\n";
        let result = dedent_content(input, "auto", None);
        assert_eq!(result, "line1\n\nline2\n");
    }

    #[test]
    fn dedent_auto_no_indent_returns_unchanged() {
        let input = "line1\nline2\n";
        let result = dedent_content(input, "auto", None);
        assert_eq!(result, input);
    }

    #[test]
    fn dedent_numeric_removes_n_spaces() {
        let input = "        line1\n    line2\n";
        let result = dedent_content(input, "4", None);
        assert_eq!(result, "    line1\nline2\n");
    }

    #[test]
    fn dedent_numeric_stops_at_available_indent() {
        let input = "  line1\n      line2\n";
        let result = dedent_content(input, "4", None);
        assert_eq!(result, "line1\n  line2\n");
    }

    #[test]
    fn dedent_tab_removes_one_tab() {
        let input = "\tline1\n\t\tline2\nline3\n";
        let result = dedent_content(input, "tab", None);
        assert_eq!(result, "line1\n\tline2\nline3\n");
    }

    #[test]
    fn dedent_tab_no_tab_unchanged() {
        let input = "    line1\n";
        let result = dedent_content(input, "tab", None);
        assert_eq!(result, "    line1\n");
    }

    #[test]
    fn dedent_auto_tab_indented_file() {
        // Auto dedent must handle tab-indented files, not just space-indented.
        let input = "\t\tline1\n\t\t\tline2\n\t\tline3\n";
        let result = dedent_content(input, "auto", None);
        assert_eq!(result, "line1\n\tline2\nline3\n");
    }

    #[test]
    fn dedent_with_line_range() {
        let input = "    line1\n    line2\n    line3\n    line4\n";
        let result = dedent_content(input, "4", Some((2, Some(3))));
        assert_eq!(result, "    line1\nline2\nline3\n    line4\n");
    }

    #[test]
    fn indent_numeric_adds_spaces() {
        let input = "line1\nline2\n";
        let result = indent_content(input, "4", None);
        assert_eq!(result, "    line1\n    line2\n");
    }

    #[test]
    fn indent_tab_adds_tab() {
        let input = "line1\nline2\n";
        let result = indent_content(input, "tab", None);
        assert_eq!(result, "\tline1\n\tline2\n");
    }

    #[test]
    fn indent_skips_blank_lines() {
        let input = "line1\n\nline2\n";
        let result = indent_content(input, "4", None);
        assert_eq!(result, "    line1\n\n    line2\n");
    }

    #[test]
    fn indent_with_line_range() {
        let input = "line1\nline2\nline3\nline4\n";
        let result = indent_content(input, "4", Some((2, Some(3))));
        assert_eq!(result, "line1\n    line2\n    line3\nline4\n");
    }

    #[test]
    fn indent_zero_returns_unchanged() {
        let input = "line1\nline2\n";
        let result = indent_content(input, "0", None);
        assert_eq!(result, input);
    }

    #[test]
    fn parse_line_range_full() {
        let (start, end) = parse_line_range("10:50").unwrap();
        assert_eq!(start, 10);
        assert_eq!(end, Some(50));
    }

    #[test]
    fn parse_line_range_open_ended() {
        let (start, end) = parse_line_range("5:").unwrap();
        assert_eq!(start, 5);
        assert_eq!(end, None);
    }

    #[test]
    fn parse_line_range_single_line() {
        let (start, end) = parse_line_range("3").unwrap();
        assert_eq!(start, 3);
        assert_eq!(end, Some(3));
    }

    #[test]
    fn parse_line_range_invalid() {
        assert!(parse_line_range("abc").is_err());
        assert!(parse_line_range("1:xyz").is_err());
    }

    #[test]
    fn parse_line_range_rejects_zero() {
        // Line numbers are 1-based; 0 is invalid.
        assert!(parse_line_range("0").is_err());
        assert!(parse_line_range("0:5").is_err());
        assert!(parse_line_range("1:0").is_err());
    }

    #[test]
    fn parse_line_range_rejects_inverted() {
        // start > end is an error, not a silent no-op.
        let err = parse_line_range("50:10").unwrap_err();
        assert!(
            err.to_string().contains("inverted"),
            "should mention inverted range: {err}"
        );
    }

    #[test]
    fn dedent_auto_line_range() {
        // Only dedent lines 2-3; leave lines 1 and 4 alone.
        let input = "    a\n        b\n        c\n    d\n";
        let result = dedent_content(input, "auto", Some((2, Some(3))));
        // min indent in range (lines 2-3) is 8 spaces, so remove 8.
        assert_eq!(result, "    a\nb\nc\n    d\n");
    }
}

mod error_handling {
    use super::*;

    #[test]
    fn atomic_create_new_fails_if_exists() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("existing.txt");
        fs::write(&target, "old").unwrap();

        let policy = WritePolicy::default();
        let err = atomic_create_new(&target, "new", &policy).unwrap_err();
        assert!(
            err.to_string().contains("already exists"),
            "error should mention 'already exists': {err}"
        );
    }

    #[test]
    fn write_policy_override_invalid_normalize_eol() {
        let mut policy = WritePolicy::default();
        let ov = WritePolicyOverride {
            normalize_eol: Some("invalid".to_string()),
            ..Default::default()
        };
        let err = policy.apply_override(&ov).unwrap_err();
        assert!(
            err.to_string().contains("invalid normalize_eol value"),
            "expected invalid eol error, got: {err}"
        );
        // Policy should not have been partially mutated before the error.
        assert!(matches!(policy.normalize_eol, EolMode::Keep));
    }
}

#[cfg(feature = "cli")]
mod shell_escape_tests {
    use super::*;

    #[test]
    fn simple_path_unchanged() {
        assert_eq!(shell_escape("src/main.rs"), "src/main.rs");
    }

    #[test]
    fn path_with_spaces_is_quoted() {
        assert_eq!(shell_escape("src/my file.rs"), "'src/my file.rs'");
    }

    #[test]
    fn path_with_single_quote_is_escaped() {
        assert_eq!(shell_escape("it's a file.rs"), "'it'\\''s a file.rs'");
    }

    #[test]
    fn dots_underscores_hyphens_slashes_safe() {
        assert_eq!(shell_escape("a-b_c.d/e"), "a-b_c.d/e");
    }
}

#[cfg(feature = "cli")]
mod format_command_tests {
    use super::*;

    #[test]
    fn no_format_flag_skips_formatting() {
        let dir = tempfile::tempdir().unwrap();
        let mut global = test_global_flags();
        global.format = Some("false".into()); // would fail if run
        global.no_format = true;
        // Should return Ok because no_format skips everything
        run_format_command(&global, dir.path()).unwrap();
    }

    #[test]
    fn run_format_command_ext_no_format_skips() {
        let dir = tempfile::tempdir().unwrap();
        let mut global = test_global_flags();
        global.no_format = true;
        let config = crate::config::FormatConfig {
            auto: Some(true),
            command: Some("false".into()),
            ..Default::default()
        };
        // no_format should prevent all formatting
        run_format_command_ext(&global, dir.path(), None, Some(&config)).unwrap();
    }

    #[test]
    fn run_format_command_ext_auto_false_skips() {
        let dir = tempfile::tempdir().unwrap();
        let global = test_global_flags();
        let config = crate::config::FormatConfig {
            auto: Some(false),
            command: Some("false".into()),
            ..Default::default()
        };
        // auto=false means skip formatting when no explicit --format
        run_format_command_ext(&global, dir.path(), None, Some(&config)).unwrap();
    }

    #[test]
    fn run_format_command_ext_auto_none_skips() {
        let dir = tempfile::tempdir().unwrap();
        let global = test_global_flags();
        let config = crate::config::FormatConfig {
            auto: None,
            command: Some("false".into()),
            ..Default::default()
        };
        run_format_command_ext(&global, dir.path(), None, Some(&config)).unwrap();
    }

    #[test]
    fn run_format_command_ext_by_extension_runs_formatter() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "hello\n").unwrap();

        let global = test_global_flags();
        let mut by_ext = std::collections::HashMap::new();
        // Use "true" as a no-op formatter that always succeeds
        by_ext.insert("txt".to_string(), "true --".to_string());
        let config = crate::config::FormatConfig {
            auto: Some(true),
            command: None,
            by_extension: by_ext,
        };
        // Should succeed (formatter "true" exits 0)
        run_format_command_ext(&global, dir.path(), Some(&["test.txt"]), Some(&config)).unwrap();
    }

    #[test]
    fn run_format_command_ext_by_extension_no_match_skips() {
        let dir = tempfile::tempdir().unwrap();
        let global = test_global_flags();
        let mut by_ext = std::collections::HashMap::new();
        by_ext.insert("rs".to_string(), "false".to_string());
        let config = crate::config::FormatConfig {
            auto: Some(true),
            command: None,
            by_extension: by_ext,
        };
        // File has .txt extension, no formatter for .txt, should be a no-op
        run_format_command_ext(&global, dir.path(), Some(&["test.txt"]), Some(&config)).unwrap();
    }

    #[test]
    fn run_format_command_ext_formatter_failure_warns_not_bails() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();

        let global = test_global_flags();
        let mut by_ext = std::collections::HashMap::new();
        // Use "false" as a formatter that always fails
        by_ext.insert("rs".to_string(), "false --".to_string());
        let config = crate::config::FormatConfig {
            auto: Some(true),
            command: None,
            by_extension: by_ext,
        };
        // Should NOT bail even though formatter fails (advisory formatting)
        run_format_command_ext(&global, dir.path(), Some(&["test.rs"]), Some(&config)).unwrap();
    }
}

mod format_preservation {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    #[cfg(unix)]
    fn atomic_write_preserves_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("script.sh");
        fs::write(&target, "#!/bin/sh\necho old\n").unwrap();

        // Set executable permission (0o755).
        fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();

        let policy = WritePolicy::default();
        atomic_write(&target, "#!/bin/sh\necho new\n", &policy).unwrap();

        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o755,
            "permissions should be preserved after atomic_write"
        );
    }

    /// Regression (#1062): when `path` is a symlink, atomic_write should not
    /// carry over the symlink target's permissions to the new regular file.
    #[test]
    #[cfg(unix)]
    fn atomic_write_symlink_does_not_carry_target_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        fs::write(&target, "content").unwrap();
        // Use 0o444 (read-only) which is distinctive from default tempfile
        // permissions (typically 0o600).
        fs::set_permissions(&target, std::fs::Permissions::from_mode(0o444)).unwrap();

        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let policy = WritePolicy::default();
        atomic_write(&link, "new content", &policy).unwrap();

        // The link should now be a regular file, not a symlink.
        assert!(!link.symlink_metadata().unwrap().file_type().is_symlink());
        // The original target should be untouched.
        assert_eq!(fs::read_to_string(&target).unwrap(), "content");
        // The new file should NOT have the target's 0o444 permissions forced
        // on it; it should have the default tempfile permissions instead.
        let mode = fs::metadata(&link).unwrap().permissions().mode() & 0o777;
        assert_ne!(
            mode, 0o444,
            "symlink target permissions should not be carried over"
        );
    }
}
