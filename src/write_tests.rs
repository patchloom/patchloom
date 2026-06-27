use super::*;
#[cfg(feature = "cli")]
use crate::cli::global::GlobalFlags;
use std::fs;

#[cfg(feature = "cli")]
fn test_global_flags() -> GlobalFlags {
    GlobalFlags::test_default()
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

mod format_preservation {
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
}
