// ── md module tests ───────────────────────────────────────────────
use crate::ops::md::*;

mod basic {
    use super::*;

    #[test]
    fn parse_headings_basic() {
        let content = "# H1\ntext\n## H2\nmore\n# H1b\n";
        let headings = parse_headings(content);
        assert_eq!(headings.len(), 3);
        assert_eq!(headings[0].level, 1);
        assert_eq!(headings[0].text, "H1");
        assert_eq!(headings[1].level, 2);
        assert_eq!(headings[1].text, "H2");
        assert_eq!(headings[2].level, 1);
        assert_eq!(headings[2].text, "H1b");
    }

    #[test]
    fn parse_headings_section_boundaries() {
        // ## B (level 2) does NOT end # A (level 1); only same-or-higher level ends it
        let content = "# A\nline1\nline2\n## B\nline3\n";
        let headings = parse_headings(content);
        assert_eq!(headings[0].line_start, 0);
        assert_eq!(headings[0].line_end, 5); // # A owns everything (no same-level heading)
        assert_eq!(headings[1].line_start, 3);
        assert_eq!(headings[1].line_end, 5); // ## B to end of content

        // Two same-level headings: second ends first
        let content2 = "# A\nbody\n# B\nmore\n";
        let h2 = parse_headings(content2);
        assert_eq!(h2[0].line_end, 2); // # A ends at # B
        assert_eq!(h2[1].line_end, 4); // # B to end
    }

    #[test]
    fn parse_headings_setext_h1() {
        let content = "Title\n=====\n\nSome content\n";
        let headings = parse_headings(content);
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].level, 1);
        assert_eq!(headings[0].text, "Title");
        assert_eq!(headings[0].line_start, 0);
    }

    #[test]
    fn parse_headings_setext_h2() {
        let content = "Subtitle\n--------\n\nMore content\n";
        let headings = parse_headings(content);
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].level, 2);
        assert_eq!(headings[0].text, "Subtitle");
        assert_eq!(headings[0].line_start, 0);
    }

    #[test]
    fn parse_headings_setext_mixed_with_atx() {
        let content = "Title\n=====\n\nSome content\n\n## ATX Heading\n\nMore text\n\nSubtitle\n--------\n\nEnd\n";
        let headings = parse_headings(content);
        assert_eq!(headings.len(), 3);
        assert_eq!(headings[0].text, "Title");
        assert_eq!(headings[0].level, 1);
        assert_eq!(headings[1].text, "ATX Heading");
        assert_eq!(headings[1].level, 2);
        assert_eq!(headings[2].text, "Subtitle");
        assert_eq!(headings[2].level, 2);
    }

    #[test]
    fn parse_headings_setext_section_operations() {
        // Verify that section-based operations work with setext headings.
        // Body must NOT include the underline.
        // Use two h1 setext headings so the first section is bounded.
        let content = "Title\n=====\n\nBody of title\n\nSubtitle\n=====\n\nBody of subtitle\n";
        let (start, end) = find_section(content, "Title").unwrap();
        let body = &content[start..end];
        assert_eq!(body, "\nBody of title\n\n", "Title body: {:?}", body);

        let (start2, end2) = find_section(content, "Subtitle").unwrap();
        let body2 = &content[start2..end2];
        assert_eq!(body2, "\nBody of subtitle\n", "Subtitle body: {:?}", body2);
    }

    #[test]
    fn find_section_returns_body_bytes() {
        // ## Next is deeper than # Title, so it's part of the section body
        let content = "# Title\nBody line 1\nBody line 2\n## Next\n";
        let (start, end) = find_section(content, "Title").unwrap();
        let body = &content[start..end];
        assert_eq!(body, "Body line 1\nBody line 2\n## Next\n");

        // Same-level heading ends the section
        let content2 = "# Title\nBody\n# Other\nKeep\n";
        let (s2, e2) = find_section(content2, "Title").unwrap();
        assert_eq!(&content2[s2..e2], "Body\n");
    }

    #[test]
    fn replace_section_basic() {
        // Use same-level heading so section boundary is clear
        let content = "# Title\nOld body\n# Next\nKeep\n";
        let result = replace_section_in(content, "Title", "New body").unwrap();
        assert_eq!(result, "# Title\nNew body\n# Next\nKeep\n");
    }

    #[test]
    fn insert_after_heading() {
        let content = "# Title\nExisting\n";
        let result = insert_after_heading_in(content, "Title", "Inserted\n").unwrap();
        assert_eq!(result, "# Title\nInserted\nExisting\n");
    }

    #[test]
    fn insert_before_heading() {
        let content = "# First\nBody\n## Second\nMore\n";
        let result = insert_before_heading_in(content, "Second", "Inserted").unwrap();
        assert!(result.contains("Inserted\n\n## Second"));
    }

    #[test]
    fn upsert_bullet_adds_new() {
        let content = "# List\n- item1\n";
        let result = upsert_bullet_in(content, "List", "- item2").unwrap();
        assert!(result.contains("- item1\n- item2\n"));
    }

    #[test]
    fn upsert_bullet_dedup_existing() {
        let content = "# List\n- item1\n";
        let result = upsert_bullet_in(content, "List", "- item1").unwrap();
        // Should return content unchanged (no duplicate)
        assert_eq!(result, content);
    }

    #[test]
    fn upsert_bullet_auto_prefix() {
        let content = "# List\n- a\n";
        let result = upsert_bullet_in(content, "List", "new item").unwrap();
        assert!(result.contains("- new item\n"));
    }

    #[test]
    fn dedupe_headings_removes_duplicate() {
        let content = "# Title\nFirst\n# Title\nSecond\n";
        let (result, removed) = dedupe_headings_in(content);
        assert_eq!(removed, vec!["# Title"]);
        // First occurrence kept, second removed
        assert!(result.contains("First"));
        assert!(!result.contains("Second"));
    }

    #[test]
    fn table_append_basic() {
        let content = "# API\n| Name | Value |\n|---|---|\n| a | 1 |\n## Next\n";
        let (start, end) = find_section(content, "API").unwrap();
        let result = table_append_in(content, start, end, "| b | 2 |").unwrap();
        assert!(result.contains("| a | 1 |\n| b | 2 |\n## Next"));
    }

    #[test]
    fn table_append_for_tx_basic() {
        let content = "# API\n| Name | Value |\n|---|---|\n| a | 1 |\n";
        let result = table_append_for_tx(content, "API", "| b | 2 |").unwrap();
        assert!(result.contains("| a | 1 |\n| b | 2 |\n"));
    }

    #[test]
    fn section_range_includes_heading() {
        let content = "# A\nbody a\n# B\nbody b\n";
        let (start, end) = section_range(content, "A").unwrap();
        assert_eq!(&content[start..end], "# A\nbody a\n");
    }

    #[test]
    fn section_range_last_section() {
        let content = "# A\nbody a\n# B\nbody b\n";
        let (start, end) = section_range(content, "B").unwrap();
        assert_eq!(&content[start..end], "# B\nbody b\n");
    }

    #[test]
    fn move_section_same_file_before() {
        let content = "# A\na body\n# B\nb body\n# C\nc body\n";
        let (result, _) = move_section_in(content, "C", content, ("before", "B"), true).unwrap();
        let a_pos = result.find("# A").unwrap();
        let c_pos = result.find("# C").unwrap();
        let b_pos = result.find("# B").unwrap();
        assert!(a_pos < c_pos);
        assert!(c_pos < b_pos);
        assert!(result.contains("a body"));
        assert!(result.contains("b body"));
        assert!(result.contains("c body"));
    }

    #[test]
    fn move_section_same_file_after() {
        let content = "# A\na body\n# B\nb body\n# C\nc body\n";
        let (result, _) = move_section_in(content, "A", content, ("after", "B"), true).unwrap();
        let b_pos = result.find("# B").unwrap();
        let a_pos = result.find("# A").unwrap();
        let c_pos = result.find("# C").unwrap();
        assert!(b_pos < a_pos);
        assert!(a_pos < c_pos);
    }

    #[test]
    fn move_section_cross_file() {
        let source = "# Keep\nkept\n# Move\nmoved\n# Stay\nstayed\n";
        let dest = "# Intro\nintro\n# End\nend\n";
        let (new_src, new_dst) =
            move_section_in(source, "Move", dest, ("before", "End"), false).unwrap();
        assert!(!new_src.contains("# Move"));
        assert!(!new_src.contains("moved"));
        assert!(new_src.contains("# Keep"));
        assert!(new_src.contains("# Stay"));
        assert!(new_dst.contains("# Move\nmoved"));
        let move_pos = new_dst.find("# Move").unwrap();
        let end_pos = new_dst.find("# End").unwrap();
        assert!(move_pos < end_pos);
    }

    #[test]
    fn move_section_preserves_sub_headings() {
        let content = "# A\n## A1\na1 content\n## A2\na2 content\n# B\nb content\n";
        let (result, _) = move_section_in(content, "A", content, ("after", "B"), true).unwrap();
        // A should now come after B, with both sub-headings preserved.
        let b_pos = result.find("# B").unwrap();
        let a_pos = result.find("# A").unwrap();
        assert!(b_pos < a_pos, "A should be after B");
        assert!(
            result.contains("## A1\na1 content"),
            "sub-heading A1 should be preserved: {result}"
        );
        assert!(
            result.contains("## A2\na2 content"),
            "sub-heading A2 should be preserved: {result}"
        );
        assert!(
            result.contains("b content"),
            "B body should be preserved: {result}"
        );
    }

    #[test]
    fn move_section_cross_file_preserves_sub_headings() {
        let source = "# A\n## A1\na1 content\n## A2\na2 content\n# B\nb content\n";
        let dest = "# X\nx content\n# Y\ny content\n";
        let (new_src, new_dst) =
            move_section_in(source, "A", dest, ("before", "Y"), false).unwrap();
        // Source should no longer have A or its sub-headings.
        assert!(!new_src.contains("# A"));
        assert!(!new_src.contains("## A1"));
        assert!(!new_src.contains("## A2"));
        assert!(new_src.contains("# B\nb content"));
        // Dest should have A with its sub-headings before Y.
        let a_pos = new_dst.find("# A").unwrap();
        let y_pos = new_dst.find("# Y").unwrap();
        assert!(a_pos < y_pos);
        assert!(new_dst.contains("## A1\na1 content"));
        assert!(new_dst.contains("## A2\na2 content"));
    }

    // ── strip_inline_code ─────────────────────────────────────────

    #[test]
    fn strip_inline_code_basic() {
        assert_eq!(strip_inline_code("use `foo` here"), "use  here");
    }

    #[test]
    fn strip_inline_code_multiple_spans() {
        assert_eq!(strip_inline_code("`a` and `b`"), " and ");
    }

    #[test]
    fn strip_inline_code_no_backticks_returns_borrowed() {
        let result = strip_inline_code("no backticks");
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
        assert_eq!(result, "no backticks");
    }

    #[test]
    fn strip_inline_code_text_around_spans() {
        assert_eq!(strip_inline_code("start `mid` end"), "start  end");
    }

    #[test]
    fn upsert_bullet_star_dedup_same_style() {
        // Upserting "* existing item" when body already has "* existing item"
        // should dedup (same style, same text).
        let content = "# List\n\n* existing item\n";
        let result = upsert_bullet_in(content, "List", "* existing item").unwrap();
        assert_eq!(result, content, "same-style duplicate should be deduped");
    }

    #[test]
    fn upsert_bullet_dash_appends_after_star_items() {
        // "- new item" appended correctly after existing "* " items.
        let content = "# List\n\n* alpha\n* beta\n";
        let result = upsert_bullet_in(content, "List", "- new item").unwrap();
        assert!(
            result.contains("* beta\n- new item\n"),
            "new dash bullet should follow existing star bullets: {:?}",
            result
        );
    }
}

mod line_endings {
    use super::*;

    // ── CRLF handling in table_append_in ──────────────────────────

    #[test]
    fn table_append_crlf_content_finds_correct_position() {
        // CRLF content: the byte-offset tracking in table_append_in
        // must correctly advance past \r\n (2 bytes) per line.
        let content = "# T\r\n| H |\r\n|---|\r\n| v |\r\n";
        let (start, end) = find_section(content, "T").unwrap();
        let result = table_append_in(content, start, end, "| new |").unwrap();
        // The new row must appear after the last data row.
        assert!(
            result.contains("| v |\r\n| new |"),
            "row should be appended after existing data row in CRLF content: {:?}",
            result
        );
        // The content before the insertion should be preserved.
        assert!(
            result.contains("| H |\r\n|---|\r\n| v |\r\n"),
            "original CRLF table should be intact: {:?}",
            result
        );
    }

    #[test]
    fn table_append_crlf_header_only() {
        // Table with only header + separator in CRLF content.
        let content = "# T\r\n| H |\r\n|---|\r\n";
        let (start, end) = find_section(content, "T").unwrap();
        let result = table_append_in(content, start, end, "| a |").unwrap();
        assert!(
            result.contains("|---|\r\n| a |"),
            "row should be appended after separator in CRLF content: {:?}",
            result
        );
    }

    #[test]
    fn table_append_crlf_multiple_data_rows() {
        // Multiple data rows with CRLF; new row goes after the last one.
        let content = "# T\r\n| H |\r\n|---|\r\n| a |\r\n| b |\r\n";
        let (start, end) = find_section(content, "T").unwrap();
        let result = table_append_in(content, start, end, "| c |").unwrap();
        assert!(
            result.contains("| b |\r\n| c |"),
            "row should be appended after the last data row: {:?}",
            result
        );
    }

    // ── CRLF in parse_headings ────────────────────────────────────

    #[test]
    fn parse_headings_crlf_strips_carriage_return() {
        // .lines() strips \r, so heading text should not contain \r.
        let content = "## Heading One\r\n\r\nBody text.\r\n";
        let headings = parse_headings(content);
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].text, "Heading One");
        assert!(
            !headings[0].text.contains('\r'),
            "heading text must not contain \\r"
        );
        assert_eq!(headings[0].level, 2);
    }

    #[test]
    fn parse_headings_crlf_multiple_headings() {
        let content = "# First\r\nBody 1\r\n## Second\r\nBody 2\r\n";
        let headings = parse_headings(content);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].text, "First");
        assert_eq!(headings[1].text, "Second");
        // Verify section boundaries are correct.
        assert_eq!(headings[0].line_start, 0);
        assert_eq!(headings[1].line_start, 2);
    }

    #[test]
    fn find_section_crlf_returns_correct_body() {
        let content = "## Heading One\r\n\r\nBody text.\r\n## Next\r\n";
        let (start, end) = find_section(content, "Heading One").unwrap();
        let body = &content[start..end];
        // Body should include the blank line and body text between headings.
        assert!(
            body.contains("Body text."),
            "body should contain body text: {:?}",
            body
        );
    }
}

mod edge_cases {
    use super::*;

    #[test]
    fn parse_headings_skips_fenced_code_blocks() {
        let content = "# Real\n```\n# Fake\n```\n## Also Real\n";
        let headings = parse_headings(content);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].text, "Real");
        assert_eq!(headings[1].text, "Also Real");
    }

    #[test]
    fn parse_headings_skips_indented_fenced_code_blocks() {
        // CommonMark allows up to 3 spaces of indentation before fence markers.
        let content = "# Real\n   ```\n# Fake inside indented fence\n   ```\n## Also Real\n";
        let headings = parse_headings(content);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].text, "Real");
        assert_eq!(headings[1].text, "Also Real");

        // 4 spaces is NOT a fence opener (indented code block instead)
        let content4 = "# Real\n    ```\n# Still Real\n    ```\n";
        let headings4 = parse_headings(content4);
        assert_eq!(headings4.len(), 2);
        assert_eq!(headings4[0].text, "Real");
        assert_eq!(headings4[1].text, "Still Real");

        // Indented tilde fences
        let tilde = "# Top\n  ~~~\n# Fake\n  ~~~\n# Bottom\n";
        let headings_tilde = parse_headings(tilde);
        assert_eq!(headings_tilde.len(), 2);
        assert_eq!(headings_tilde[0].text, "Top");
        assert_eq!(headings_tilde[1].text, "Bottom");
    }

    #[test]
    fn parse_headings_longer_fence_requires_matching_length() {
        // A 4-backtick fence is only closed by 4+ backticks, not 3.
        let content = "# Top\n````\n```\n# Fake\n```\n````\n# Bottom\n";
        let headings = parse_headings(content);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].text, "Top");
        assert_eq!(headings[1].text, "Bottom");

        // A 5-tilde fence requires 5+ tildes to close
        let tilde5 = "# Top\n~~~~~\n~~~\n# Fake\n~~~\n~~~~~\n# Bottom\n";
        let headings5 = parse_headings(tilde5);
        assert_eq!(headings5.len(), 2);
        assert_eq!(headings5[0].text, "Top");
        assert_eq!(headings5[1].text, "Bottom");
    }

    #[test]
    fn parse_headings_mixed_fence_markers() {
        // ~~~ inside a ``` block is content, not a closer
        let content = "# Top\n```\n~~~\n# Not Real\n~~~\n```\n# Bottom\n";
        let headings = parse_headings(content);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].text, "Top");
        assert_eq!(headings[1].text, "Bottom");
    }

    #[test]
    fn parse_headings_skips_tilde_fenced_blocks() {
        let content = "# Top\n~~~bash\n# Not a heading\n~~~\n# Bottom\n";
        let headings = parse_headings(content);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].text, "Top");
        assert_eq!(headings[1].text, "Bottom");
    }

    #[test]
    fn parse_headings_setext_inside_fenced_code_block_ignored() {
        let content = "# Real\n```\nFake\n====\n```\n## Also Real\n";
        let headings = parse_headings(content);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].text, "Real");
        assert_eq!(headings[1].text, "Also Real");
    }

    #[test]
    fn parse_headings_setext_single_char_underline() {
        // CommonMark allows a single = or - as an underline.
        let content = "H1\n=\n\nH2\n-\n";
        let headings = parse_headings(content);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].level, 1);
        assert_eq!(headings[0].text, "H1");
        assert_eq!(headings[1].level, 2);
        assert_eq!(headings[1].text, "H2");
    }

    #[test]
    fn parse_headings_setext_not_triggered_by_blank_preceding_line() {
        // A --- after a blank line is a thematic break, not a setext heading.
        let content = "Some text\n\n---\n\nMore text\n";
        let headings = parse_headings(content);
        assert!(
            headings.is_empty(),
            "thematic break should not create a heading: {:?}",
            headings
        );
    }

    #[test]
    fn parse_headings_ignores_invalid() {
        let content = "#nospace\n##also\n# Valid\n###### Six\n####### Seven\n";
        let headings = parse_headings(content);
        // Only "# Valid" and "###### Six" are valid (Seven > 6 levels)
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].text, "Valid");
        assert_eq!(headings[1].text, "Six");
    }

    #[test]
    fn parse_headings_strips_atx_closing_hashes() {
        let content = "## Heading ##\n### Another ###\n";
        let headings = parse_headings(content);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].text, "Heading");
        assert_eq!(headings[1].text, "Another");
    }

    #[test]
    fn parse_headings_closing_hashes_without_space_are_content() {
        // Per CommonMark, closing hashes must be preceded by a space.
        let content = "# foo#\n## bar##baz\n";
        let headings = parse_headings(content);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].text, "foo#");
        assert_eq!(headings[1].text, "bar##baz");
    }

    #[test]
    fn find_section_matches_heading_with_closing_hashes() {
        let content = "## API ##\nsome text\n## Next\nother\n";
        let (start, end) = find_section(content, "API").unwrap();
        assert_eq!(&content[start..end], "some text\n");
    }

    #[test]
    fn find_section_with_hashes_in_query() {
        let content = "## API\nsome text\n";
        let (start, end) = find_section(content, "## API").unwrap();
        assert_eq!(&content[start..end], "some text\n");
    }

    #[test]
    fn replace_section_empty_replacement() {
        let content = "# Title\nOld body\n# Next\nKeep\n";
        let result = replace_section_in(content, "Title", "").unwrap();
        assert_eq!(result, "# Title\n# Next\nKeep\n");
    }

    #[test]
    fn insert_after_heading_places_content_under_heading_not_after_body() {
        // Explicit test + documentation: insert_after_heading inserts
        // immediately after the heading (before any existing body content
        // such as tables). This is distinct from "move after" which uses
        // the full destination body end.
        let content =
            "## Features\n\n| Name | Status |\n|------|--------|\n| search | done |\n\n## Other\n";
        let result = insert_after_heading_in(content, "## Features", "New intro.\n").unwrap();
        let f = result.find("## Features").unwrap();
        let i = result.find("New intro").unwrap();
        let t = result.find("| Name | Status |").unwrap();
        assert!(f < i && i < t);
        assert!(result.contains("| search | done |"));
    }

    #[test]
    fn dedupe_headings_no_duplicates() {
        let content = "# A\n## B\n# C\n";
        let (result, removed) = dedupe_headings_in(content);
        assert!(removed.is_empty());
        assert_eq!(result, content);
    }

    #[test]
    fn table_append_header_only() {
        let content = "# API\n| Name | Value |\n|---|---|\n## Next\n";
        let (start, end) = find_section(content, "API").unwrap();
        let result = table_append_in(content, start, end, "| a | 1 |").unwrap();
        assert_eq!(
            result,
            "# API\n| Name | Value |\n|---|---|\n| a | 1 |\n## Next\n"
        );
    }

    #[test]
    fn move_section_self_move_before_returns_none() {
        // Moving section A to before itself: after removing A, the
        // destination heading no longer exists, so the move returns None.
        let content = "# A\na body\n# B\nb body\n";
        let result = move_section_in(content, "A", content, ("before", "A"), true);
        assert!(
            result.is_none(),
            "self-move (before self) should return None: {:?}",
            result
        );
    }

    #[test]
    fn move_section_self_move_after_returns_none() {
        // Moving section A to after itself: same issue.
        let content = "# A\na body\n# B\nb body\n";
        let result = move_section_in(content, "A", content, ("after", "A"), true);
        assert!(
            result.is_none(),
            "self-move (after self) should return None: {:?}",
            result
        );
    }

    #[test]
    fn strip_inline_code_unmatched_backtick() {
        assert_eq!(strip_inline_code("before `after"), "before after");
    }

    #[test]
    fn strip_inline_code_empty_string() {
        let result = strip_inline_code("");
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
        assert_eq!(result, "");
    }

    #[test]
    fn strip_inline_code_adjacent_backticks() {
        assert_eq!(strip_inline_code("``"), "");
    }

    #[test]
    fn strip_inline_code_only_backtick_content() {
        assert_eq!(strip_inline_code("`code`"), "");
    }

    #[test]
    fn git_add_dot_empty_string() {
        assert!(!has_dangerous_git_add_dot(""));
    }

    #[test]
    fn upsert_bullet_cross_style_dash_into_star_dedup() {
        // Upserting "- existing item" when body has "* existing item":
        // cross-style dedup should recognize them as the same item.
        let content = "# List\n\n* existing item\n";
        let result = upsert_bullet_in(content, "List", "- existing item").unwrap();
        assert_eq!(result, content, "cross-style bullets should dedup");
    }

    #[test]
    fn upsert_bullet_no_prefix_into_star_content_dedup() {
        // Upserting "existing item" (no prefix) normalizes to "- existing item"
        // which should dedup against body's "* existing item".
        let content = "# List\n\n* existing item\n";
        let result = upsert_bullet_in(content, "List", "existing item").unwrap();
        assert_eq!(
            result, content,
            "auto-prefixed dash should dedup against star bullet"
        );
    }

    #[test]
    fn upsert_bullet_plus_prefix_dedup_against_dash() {
        let content = "# List\n\n- existing item\n";
        let result = upsert_bullet_in(content, "List", "+ existing item").unwrap();
        assert_eq!(
            result, content,
            "plus-prefixed bullet should dedup against dash"
        );
    }

    #[test]
    fn find_section_empty_body() {
        // Heading immediately followed by another heading => empty body
        let content = "# A\n# B\n";
        let (start, end) = find_section(content, "A").unwrap();
        assert_eq!(
            start, end,
            "empty section should have body_start == body_end"
        );
        // The body slice should be empty
        assert_eq!(&content[start..end], "");
    }

    #[test]
    fn find_section_empty_body_with_subsection() {
        // ## level heading immediately after # level => # A body includes ## B
        // but if both are same level, body is empty
        let content = "## A\n## B\nB body\n";
        let (start, end) = find_section(content, "A").unwrap();
        assert_eq!(&content[start..end], "");
    }

    #[test]
    fn replace_section_empty_body_inserts_content() {
        let content = "# A\n# B\n";
        let result = replace_section_in(content, "A", "new content").unwrap();
        assert_eq!(result, "# A\nnew content\n# B\n");
    }

    #[test]
    fn replace_section_empty_body_with_empty_replacement() {
        let content = "# A\n# B\n";
        let result = replace_section_in(content, "A", "").unwrap();
        assert_eq!(result, "# A\n# B\n");
    }

    #[test]
    fn insert_after_heading_empty_section() {
        let content = "# A\n# B\n";
        let result = insert_after_heading_in(content, "A", "inserted\n").unwrap();
        assert_eq!(result, "# A\ninserted\n# B\n");
    }

    #[test]
    fn insert_after_heading_empty_section_no_trailing_newline() {
        let content = "# A\n# B\n";
        let result = insert_after_heading_in(content, "A", "inserted").unwrap();
        // Function appends \n when insertion doesn't end with one
        assert!(result.contains("# A\ninserted\n# B\n"));
    }

    #[test]
    fn upsert_bullet_empty_section() {
        let content = "# A\n# B\n";
        let result = upsert_bullet_in(content, "A", "- new bullet").unwrap();
        assert!(
            result.contains("- new bullet"),
            "bullet should appear in result: {result}"
        );
        assert!(
            result.contains("# B"),
            "next heading should be preserved: {result}"
        );
    }

    #[test]
    fn upsert_bullet_empty_section_auto_prefix() {
        let content = "# A\n# B\n";
        let result = upsert_bullet_in(content, "A", "item without dash").unwrap();
        assert!(
            result.contains("- item without dash"),
            "auto-prefix should be added: {result}"
        );
    }

    #[test]
    fn find_section_duplicate_headings_returns_first() {
        let content = "# A\nfirst body\n# B\nmiddle\n# A\nsecond body\n";
        let (start, end) = find_section(content, "A").unwrap();
        let body = &content[start..end];
        assert_eq!(body, "first body\n");
    }

    #[test]
    fn find_section_eof_without_trailing_newline() {
        let content = "# A\ncontent";
        let (start, end) = find_section(content, "A").unwrap();
        let body = &content[start..end];
        assert_eq!(body, "content");
    }

    #[test]
    fn find_section_single_heading_no_body_no_newline() {
        let content = "# A";
        let (start, end) = find_section(content, "A").unwrap();
        assert_eq!(start, end);
        assert_eq!(&content[start..end], "");
    }

    #[test]
    fn replace_section_eof_without_trailing_newline() {
        let content = "# A\nold content";
        let result = replace_section_in(content, "A", "new content").unwrap();
        assert_eq!(result, "# A\nnew content\n");
    }

    #[test]
    fn insert_after_heading_at_eof() {
        let content = "# A\nexisting";
        let result = insert_after_heading_in(content, "A", "inserted\n").unwrap();
        assert_eq!(result, "# A\ninserted\nexisting");
    }

    #[test]
    fn empty_section_three_consecutive_headings() {
        // Both A and B are empty; C has body
        let content = "# A\n# B\n# C\nbody\n";
        let (a_start, a_end) = find_section(content, "A").unwrap();
        assert_eq!(&content[a_start..a_end], "");

        let (b_start, b_end) = find_section(content, "B").unwrap();
        assert_eq!(&content[b_start..b_end], "");

        let (c_start, c_end) = find_section(content, "C").unwrap();
        assert_eq!(&content[c_start..c_end], "body\n");
    }
}

mod error_handling {
    use super::*;

    #[test]
    fn find_section_missing() {
        let content = "# Title\nBody\n";
        assert!(find_section(content, "Nonexistent").is_none());
    }

    #[test]
    fn replace_section_missing_heading() {
        let content = "# Title\nBody\n";
        assert!(replace_section_in(content, "Missing", "x").is_none());
    }

    #[test]
    fn table_append_no_table() {
        let content = "# API\nJust text\n";
        let (start, end) = find_section(content, "API").unwrap();
        assert!(table_append_in(content, start, end, "| b | 2 |").is_none());
    }

    #[test]
    fn section_range_missing() {
        let content = "# A\nbody\n";
        assert!(section_range(content, "Missing").is_none());
    }

    #[test]
    fn move_section_missing_source_heading() {
        let content = "# A\nbody\n";
        assert!(move_section_in(content, "Missing", content, ("before", "A"), true).is_none());
    }

    #[test]
    fn move_section_missing_target_heading() {
        let content = "# A\nbody\n# B\nbody\n";
        assert!(move_section_in(content, "A", content, ("before", "Missing"), true).is_none());
    }
}

mod security {
    use super::*;

    // ── has_dangerous_git_add_dot ─────────────────────────────────

    #[test]
    fn git_add_dot_at_eol() {
        assert!(has_dangerous_git_add_dot("git add ."));
    }

    #[test]
    fn git_add_dot_followed_by_space() {
        assert!(has_dangerous_git_add_dot("git add . && git commit"));
    }

    #[test]
    fn git_add_dot_followed_by_tab() {
        assert!(has_dangerous_git_add_dot("git add .\tnext"));
    }

    #[test]
    fn git_add_dotgitignore_is_safe() {
        assert!(!has_dangerous_git_add_dot("git add .gitignore"));
    }

    #[test]
    fn git_add_dot_slash_is_safe() {
        assert!(!has_dangerous_git_add_dot("git add ./file"));
    }

    #[test]
    fn git_add_dot_mid_line() {
        assert!(has_dangerous_git_add_dot("run git add . now"));
    }

    #[test]
    fn git_add_dot_no_match() {
        assert!(!has_dangerous_git_add_dot("no match here"));
    }

    #[test]
    fn git_add_dot_multiple_occurrences_first_safe() {
        // First "git add .gitignore" is safe, second "git add ." is dangerous
        assert!(has_dangerous_git_add_dot("git add .gitignore && git add ."));
    }

    #[test]
    fn git_add_dot_multiple_occurrences_all_safe() {
        assert!(!has_dangerous_git_add_dot(
            "git add .gitignore && git add .env"
        ));
    }
}

mod format_preservation {
    use super::*;

    #[test]
    fn replace_section_setext_preserves_underline() {
        // Use same-level headings so the section is bounded.
        let content = "Title\n=====\n\nOld body\n\nNext\n=====\nKeep\n";
        let result = replace_section_in(content, "Title", "New body").unwrap();
        assert_eq!(
            result, "Title\n=====\nNew body\nNext\n=====\nKeep\n",
            "underline must be preserved: {result}"
        );
    }

    #[test]
    fn upsert_bullet_setext_heading() {
        let content = "List\n----\n\n- item1\n";
        let result = upsert_bullet_in(content, "List", "- item2").unwrap();
        assert!(
            result.contains("----\n"),
            "underline must be preserved: {result}"
        );
        assert!(
            result.contains("- item1\n- item2\n"),
            "bullet should be appended: {result}"
        );
        // Underline must NOT appear in the body match
        assert!(
            !result.contains("- ----"),
            "underline must not be treated as bullet content: {result}"
        );
    }

    #[test]
    fn section_range_setext_includes_text_and_underline() {
        // Use same-level heading to bound the section.
        let content = "Title\n=====\nbody\nNext\n=====\nmore\n";
        let (start, end) = section_range(content, "Title").unwrap();
        // section_range should include the text line + underline + body
        assert_eq!(
            &content[start..end],
            "Title\n=====\nbody\n",
            "setext section_range should include text line and underline"
        );
    }

    // ── Mixed bullet styles in upsert_bullet_in ──────────────────

    #[test]
    fn upsert_bullet_star_prefix_kept_as_is() {
        // When input starts with "* ", it is kept as-is (not converted to "- ").
        let content = "# List\n\n* existing item\n";
        let result = upsert_bullet_in(content, "List", "* new item").unwrap();
        assert!(
            result.contains("* new item"),
            "star-prefixed bullet should be preserved: {:?}",
            result
        );
    }

    #[test]
    fn upsert_bullet_plus_prefix_preserved() {
        // When inserting a new bullet with +, the prefix is kept.
        let content = "# List\n\n- other item\n";
        let result = upsert_bullet_in(content, "List", "+ brand new").unwrap();
        assert!(
            result.contains("+ brand new"),
            "plus-prefixed bullet should be preserved: {:?}",
            result
        );
    }
}

mod regression {
    use super::*;

    #[test]
    fn upsert_bullet_preserves_blank_line_before_next_heading() {
        // Regression test for #973: upsert_bullet consumed the blank line
        // separating the section body from the next heading.
        let content = "## Section A\n\n- Bullet one\n- Bullet two\n\n## Section B\n\nContent B.\n";
        let result = upsert_bullet_in(content, "Section A", "- Bullet three").unwrap();
        assert!(
            result.contains("- Bullet three\n\n## Section B"),
            "blank line before next heading must be preserved: {result}"
        );
    }

    #[test]
    fn upsert_bullet_preserves_blank_before_sub_heading() {
        // #973 variant: subsection headings (###) also need blank line preservation.
        let content = "### Bug Fixes\n\n- Fix one\n\n### Dependencies\n\nDep content.\n";
        let result = upsert_bullet_in(content, "Bug Fixes", "- Fix two").unwrap();
        assert!(
            result.contains("- Fix two\n\n### Dependencies"),
            "blank line before sub-heading must be preserved: {result}"
        );
    }

    #[test]
    fn move_section_after_preserves_dest_table_body() {
        // Regression for #825: moving a section "after" a dest that owns a
        // table must keep the table under the dest heading.
        let content = "# T\n\n## Commands\n\ncmd body\n\n## Rules\n\n- rule1\n\n## Features\n\n| Name | Status |\n|------|--------|\n| search | done |\n";
        let (result, _) =
            move_section_in(content, "## Rules", content, ("after", "## Features"), true).unwrap();
        // Features heading should still be followed (eventually) by its table,
        // and Rules after the table.
        let features_pos = result.find("## Features").unwrap();
        let rules_pos = result.find("## Rules").unwrap();
        let table_pos = result.find("| search | done |").unwrap();
        assert!(
            features_pos < table_pos,
            "table should still be after Features"
        );
        assert!(table_pos < rules_pos, "Rules should be after the table");
        assert!(result.contains("- rule1"));
    }

    // Regression: backtick closing fence with trailing non-whitespace
    // must NOT close the code block (CommonMark spec 4.5). A line like
    // "```javascript" inside a backtick block is not a valid closer.
    #[test]
    fn backtick_fence_closer_rejects_trailing_content() {
        let content = "\
````
```javascript
# Not a real heading
```
````
# Real heading
body text
";
        let headings = parse_headings(content);
        assert_eq!(
            headings.len(),
            1,
            "only the heading outside the code block should be parsed"
        );
        assert_eq!(headings[0].text, "Real heading");
    }

    // Tilde fences ARE allowed to have trailing content on the closing fence
    // per CommonMark spec (only backtick fences have this restriction).
    #[test]
    fn tilde_fence_closer_accepts_trailing_content() {
        let content = "\
~~~
# Inside tilde block
~~~ some trailing text
# Outside heading
";
        let headings = parse_headings(content);
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].text, "Outside heading");
    }

    // Verify that a clean backtick closing fence (no trailing content) still works.
    #[test]
    fn backtick_fence_closer_allows_trailing_whitespace() {
        let content = "\
```
# Inside code block
```   \t
# Real heading
";
        let headings = parse_headings(content);
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].text, "Real heading");
    }
}
