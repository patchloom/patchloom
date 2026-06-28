use crate::ops::patch::*;

/// Helper: build a hunk with prefix context, removes, adds, suffix context.
fn make_hunk(
    old_start: usize,
    prefix: &[&str],
    removes: &[&str],
    adds: &[&str],
    suffix: &[&str],
) -> Hunk {
    let mut lines = Vec::new();
    for s in prefix {
        lines.push(PatchLine::Context(s.to_string()));
    }
    for s in removes {
        lines.push(PatchLine::Remove(s.to_string()));
    }
    for s in adds {
        lines.push(PatchLine::Add(s.to_string()));
    }
    for s in suffix {
        lines.push(PatchLine::Context(s.to_string()));
    }
    let old_count = prefix.len() + removes.len() + suffix.len();
    let new_count = prefix.len() + adds.len() + suffix.len();
    Hunk {
        old_start,
        old_count,
        new_start: old_start,
        new_count,
        lines,
    }
}

/// Shorthand: `&[&str]` → `Vec<String>`.
fn s(strings: &[&str]) -> Vec<String> {
    strings.iter().map(|s| s.to_string()).collect()
}

mod basic {
    use super::*;

    #[test]
    fn parse_patch_single_file() {
        let diff = "\
--- a/hello.txt
+++ b/hello.txt
@@ -1,3 +1,3 @@
 line1
-line2
+LINE2
 line3
";
        let files = parse_patch(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "hello.txt");
        assert_eq!(files[0].hunks.len(), 1);
        assert_eq!(files[0].hunks[0].old_start, 1);
        assert_eq!(files[0].hunks[0].old_count, 3);
    }

    #[test]
    fn parse_patch_multiple_files() {
        let diff = "\
--- a/a.txt
+++ b/a.txt
@@ -1,1 +1,1 @@
-old
+new
--- a/b.txt
+++ b/b.txt
@@ -1,1 +1,1 @@
-foo
+bar
";
        let files = parse_patch(diff).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "a.txt");
        assert_eq!(files[1].path, "b.txt");
    }

    #[test]
    fn apply_hunks_simple_replacement() {
        let original = "line1\nline2\nline3\n";
        let hunks = vec![Hunk {
            old_start: 2,
            old_count: 1,
            new_start: 2,
            new_count: 1,
            lines: vec![
                PatchLine::Context("line1".into()),
                PatchLine::Remove("line2".into()),
                PatchLine::Add("LINE2".into()),
                PatchLine::Context("line3".into()),
            ],
        }];
        let result = apply_hunks(original, &hunks).unwrap();
        assert_eq!(result, "line1\nLINE2\nline3\n");
    }

    #[test]
    fn apply_hunks_addition() {
        let original = "a\nb\n";
        let hunks = vec![Hunk {
            old_start: 1,
            old_count: 2,
            new_start: 1,
            new_count: 3,
            lines: vec![
                PatchLine::Context("a".into()),
                PatchLine::Add("inserted".into()),
                PatchLine::Context("b".into()),
            ],
        }];
        let result = apply_hunks(original, &hunks).unwrap();
        assert_eq!(result, "a\ninserted\nb\n");
    }

    #[test]
    fn apply_hunks_deletion() {
        let original = "a\nremove_me\nb\n";
        let hunks = vec![Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 2,
            lines: vec![
                PatchLine::Context("a".into()),
                PatchLine::Remove("remove_me".into()),
                PatchLine::Context("b".into()),
            ],
        }];
        let result = apply_hunks(original, &hunks).unwrap();
        assert_eq!(result, "a\nb\n");
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn apply_patch_with_loader_basic() {
        let diff = "\
--- a/test.txt
+++ b/test.txt
@@ -1,3 +1,3 @@
 hello
-world
+WORLD
 end
";
        let results = apply_patch_with_loader(
            diff,
            |path| {
                assert_eq!(path, "test.txt");
                Ok("hello\nworld\nend\n".to_string())
            },
            ApplyHunksOptions::default(),
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "test.txt");
        assert_eq!(results[0].content, "hello\nWORLD\nend\n");
    }

    #[test]
    fn apply_hunks_two_hunks_offset_tracking() {
        // First hunk adds a line (shifting later content down), second
        // hunk must correctly account for the offset.
        let original = "a\nb\nc\nd\ne\n";
        let hunks = vec![
            Hunk {
                old_start: 1,
                old_count: 2,
                new_start: 1,
                new_count: 3,
                lines: vec![
                    PatchLine::Context("a".into()),
                    PatchLine::Add("INSERTED".into()),
                    PatchLine::Context("b".into()),
                ],
            },
            Hunk {
                old_start: 4,
                old_count: 2,
                new_start: 5,
                new_count: 2,
                lines: vec![
                    PatchLine::Remove("d".into()),
                    PatchLine::Add("D".into()),
                    PatchLine::Context("e".into()),
                ],
            },
        ];
        let result = apply_hunks(original, &hunks).unwrap();
        assert_eq!(result, "a\nINSERTED\nb\nc\nD\ne\n");
    }

    #[test]
    fn merge_three_way_lines_theirs_wins() {
        // ours == base for the differing line → take theirs
        let base = s(&["same", "base_val", "same"]);
        let ours = s(&["same", "base_val", "same"]);
        let theirs = s(&["same", "theirs_val", "same"]);
        let (result, conflicts) = merge_three_way_lines(&base, &ours, &theirs, 1);
        assert_eq!(result, s(&["same", "theirs_val", "same"]));
        assert!(conflicts.is_empty());
    }

    #[test]
    fn merge_three_way_lines_ours_wins_theirs_unchanged() {
        // theirs == base, ours changed → take ours
        let base = s(&["A", "base", "C"]);
        let ours = s(&["A", "ours", "C"]);
        let theirs = s(&["A", "base", "C"]);
        let (result, conflicts) = merge_three_way_lines(&base, &ours, &theirs, 1);
        assert_eq!(result, s(&["A", "ours", "C"]));
        assert!(conflicts.is_empty());
    }

    #[test]
    fn merge_three_way_lines_ours_wins_same_change() {
        // ours == theirs (both changed identically) → take ours, no conflict
        let base = s(&["X"]);
        let ours = s(&["Z"]);
        let theirs = s(&["Z"]);
        let (result, conflicts) = merge_three_way_lines(&base, &ours, &theirs, 1);
        assert_eq!(result, s(&["Z"]));
        assert!(conflicts.is_empty());
    }

    #[test]
    fn merge_three_way_lines_conflict() {
        // base, ours, theirs all differ → conflict markers
        let base = s(&["base"]);
        let ours = s(&["ours"]);
        let theirs = s(&["theirs"]);
        let (result, conflicts) = merge_three_way_lines(&base, &ours, &theirs, 1);
        assert_eq!(result.len(), 5);
        assert_eq!(result[0], CONFLICT_OURS);
        assert_eq!(result[1], "ours");
        assert_eq!(result[2], CONFLICT_SEP);
        assert_eq!(result[3], "theirs");
        assert_eq!(result[4], CONFLICT_THEIRS);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].start_line, 1);
        assert_eq!(conflicts[0].end_line, 5);
    }

    #[test]
    fn merge_three_way_lines_mixed_wins() {
        // Line 1: ours changed, theirs unchanged → ours wins
        // Line 2: ours unchanged, theirs changed → theirs wins
        let base = s(&["B1", "B2"]);
        let ours = s(&["O1", "B2"]);
        let theirs = s(&["B1", "T2"]);
        let (result, conflicts) = merge_three_way_lines(&base, &ours, &theirs, 1);
        assert_eq!(result, s(&["O1", "T2"]));
        assert!(conflicts.is_empty());
    }

    #[test]
    fn merge_three_way_block_theirs_wins() {
        // ours == base (unchanged), theirs adds a line → take theirs
        let base = s(&["A", "B"]);
        let ours = s(&["A", "B"]);
        let theirs = s(&["A", "B", "C"]);
        let (result, conflicts) = merge_three_way_block(&base, &ours, &theirs, 1);
        assert_eq!(result, s(&["A", "B", "C"]));
        assert!(conflicts.is_empty());
    }

    #[test]
    fn merge_three_way_block_ours_wins() {
        // theirs == base (unchanged), ours adds a line → take ours
        let base = s(&["A", "B"]);
        let ours = s(&["A", "B", "new"]);
        let theirs = s(&["A", "B"]);
        let (result, conflicts) = merge_three_way_block(&base, &ours, &theirs, 1);
        assert_eq!(result, s(&["A", "B", "new"]));
        assert!(conflicts.is_empty());
    }

    #[test]
    fn merge_three_way_block_conflict() {
        // All three differ (different lengths) → block conflict with markers
        let base = s(&["B1", "B2"]);
        let ours = s(&["O1", "O2", "O3"]);
        let theirs = s(&["T1", "T2", "T3", "T4"]);
        let (result, conflicts) = merge_three_way_block(&base, &ours, &theirs, 1);
        assert_eq!(result[0], CONFLICT_OURS);
        assert_eq!(result[1], "O1");
        assert_eq!(result[2], "O2");
        assert_eq!(result[3], "O3");
        assert_eq!(result[4], CONFLICT_SEP);
        assert_eq!(result[5], "T1");
        assert_eq!(result[6], "T2");
        assert_eq!(result[7], "T3");
        assert_eq!(result[8], "T4");
        assert_eq!(result[9], CONFLICT_THEIRS);
        assert_eq!(result.len(), 10);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].start_line, 1);
        assert_eq!(conflicts[0].end_line, 10);
    }

    #[test]
    fn find_match_global_finds_at_start() {
        let haystack: Vec<&str> = vec!["A", "B", "C", "D"];
        let needle: Vec<&str> = vec!["A", "B"];
        assert_eq!(find_match_global(&haystack, &needle), Some(0));
    }

    #[test]
    fn find_match_global_finds_in_middle() {
        let haystack: Vec<&str> = vec!["X", "A", "B", "Y"];
        let needle: Vec<&str> = vec!["A", "B"];
        assert_eq!(find_match_global(&haystack, &needle), Some(1));
    }

    #[test]
    fn find_match_global_finds_at_end() {
        let haystack: Vec<&str> = vec!["X", "Y", "A", "B"];
        let needle: Vec<&str> = vec!["A", "B"];
        assert_eq!(find_match_global(&haystack, &needle), Some(2));
    }

    #[test]
    fn locate_by_context_anchors_prefix_only() {
        // Hunk has prefix context but no suffix → locates via prefix
        let hunk = Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                PatchLine::Context("ctx1".into()),
                PatchLine::Context("ctx2".into()),
                PatchLine::Remove("old".into()),
                PatchLine::Add("new".into()),
            ],
        };
        let haystack: Vec<&str> = vec!["ctx1", "ctx2", "modified"];
        let result = locate_by_context_anchors(&haystack, &hunk, 0);
        assert_eq!(result, Some(0));
    }

    #[test]
    fn locate_by_context_anchors_suffix_only() {
        // Hunk has suffix context but no prefix → locates via suffix
        let hunk = Hunk {
            old_start: 1,
            old_count: 2,
            new_start: 1,
            new_count: 2,
            lines: vec![
                PatchLine::Remove("old".into()),
                PatchLine::Add("new".into()),
                PatchLine::Context("suffix".into()),
            ],
        };
        let haystack: Vec<&str> = vec!["modified", "suffix"];
        let result = locate_by_context_anchors(&haystack, &hunk, 0);
        assert_eq!(result, Some(0));
    }

    #[test]
    fn locate_by_context_anchors_both() {
        // Hunk has both prefix and suffix context
        let hunk = Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                PatchLine::Context("prefix".into()),
                PatchLine::Remove("old".into()),
                PatchLine::Add("new".into()),
                PatchLine::Context("suffix".into()),
            ],
        };
        let haystack: Vec<&str> = vec!["prefix", "modified", "suffix"];
        let result = locate_by_context_anchors(&haystack, &hunk, 0);
        assert_eq!(result, Some(0));
    }

    #[test]
    fn merge_hunks_clean_apply() {
        // ours region matches old_refs exactly → direct apply, no three-way
        let ours = "ctx1\nold\nctx2\n";
        let hunks = vec![make_hunk(1, &["ctx1"], &["old"], &["new"], &["ctx2"])];
        let result = merge_hunks(ours, &hunks).unwrap();
        assert_eq!(result.content, "ctx1\nnew\nctx2\n");
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn merge_hunks_three_way_conflict() {
        // ours modified the same line differently from theirs → conflict
        let ours = "ctx1\nours_modified\nctx2\n";
        let hunks = vec![make_hunk(
            1,
            &["ctx1"],
            &["base_val"],
            &["theirs_val"],
            &["ctx2"],
        )];
        let result = merge_hunks(ours, &hunks).unwrap();
        assert!(!result.conflicts.is_empty());
        assert!(result.content.contains(CONFLICT_OURS));
        assert!(result.content.contains("ours_modified"));
        assert!(result.content.contains("theirs_val"));
        assert!(result.content.contains(CONFLICT_THEIRS));
    }

    #[test]
    fn merge_hunks_three_way_clean() {
        // ours changed one line, theirs changed a different line → clean merge
        let ours = "ctx1\nours_val\nchange_this\nctx2\n";
        let hunks = vec![Hunk {
            old_start: 1,
            old_count: 4,
            new_start: 1,
            new_count: 4,
            lines: vec![
                PatchLine::Context("ctx1".into()),
                PatchLine::Remove("keep_this".into()),
                PatchLine::Remove("change_this".into()),
                PatchLine::Add("keep_this".into()),
                PatchLine::Add("patched".into()),
                PatchLine::Context("ctx2".into()),
            ],
        }];
        let result = merge_hunks(ours, &hunks).unwrap();
        // Line 2: ours changed "keep_this" → "ours_val" (theirs kept it) → ours wins
        // Line 3: ours kept "change_this" (theirs changed it) → theirs wins → "patched"
        assert_eq!(result.content, "ctx1\nours_val\npatched\nctx2\n");
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn apply_with_options_merge_clean() {
        // Content matches perfectly → Clean status even in Merge mode
        let ours = "ctx1\nold\nctx2\n";
        let hunks = vec![make_hunk(1, &["ctx1"], &["old"], &["new"], &["ctx2"])];
        let opts = ApplyHunksOptions {
            on_stale: OnStale::Merge,
            allow_conflicts: false,
        };
        let result = apply_hunks_with_options(ours, &hunks, opts).unwrap();
        assert_eq!(result.status, ApplyHunksStatus::Clean);
        assert_eq!(result.content, "ctx1\nnew\nctx2\n");
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn apply_with_options_merge_merged() {
        // Stale context but merge succeeds without conflicts → Merged
        let ours = "ctx1\nours_val\nchange_this\nctx2\n";
        let hunks = vec![Hunk {
            old_start: 1,
            old_count: 4,
            new_start: 1,
            new_count: 4,
            lines: vec![
                PatchLine::Context("ctx1".into()),
                PatchLine::Remove("keep_this".into()),
                PatchLine::Remove("change_this".into()),
                PatchLine::Add("keep_this".into()),
                PatchLine::Add("patched".into()),
                PatchLine::Context("ctx2".into()),
            ],
        }];
        let opts = ApplyHunksOptions {
            on_stale: OnStale::Merge,
            allow_conflicts: false,
        };
        let result = apply_hunks_with_options(ours, &hunks, opts).unwrap();
        assert_eq!(result.status, ApplyHunksStatus::Merged);
        assert!(result.conflicts.is_empty());
        assert_eq!(result.content, "ctx1\nours_val\npatched\nctx2\n");
    }

    #[test]
    fn apply_with_options_merge_conflict_allowed() {
        // Stale content, conflicting merge, allow_conflicts = true → Conflict
        let ours = "ctx1\nours_mod\nctx2\n";
        let hunks = vec![make_hunk(1, &["ctx1"], &["base"], &["theirs"], &["ctx2"])];
        let opts = ApplyHunksOptions {
            on_stale: OnStale::Merge,
            allow_conflicts: true,
        };
        let result = apply_hunks_with_options(ours, &hunks, opts).unwrap();
        assert_eq!(result.status, ApplyHunksStatus::Conflict);
        assert!(!result.conflicts.is_empty());
        assert!(result.content.contains(CONFLICT_OURS));
        assert!(result.content.contains("ours_mod"));
        assert!(result.content.contains("theirs"));
        assert!(result.content.contains(CONFLICT_THEIRS));
    }

    #[test]
    fn parse_patch_with_diff_git_headers() {
        let diff = "\
diff --git a/src/hello.rs b/src/hello.rs
index 1234567..abcdefg 100644
--- a/src/hello.rs
+++ b/src/hello.rs
@@ -1,3 +1,3 @@
 fn main() {
-    println!(\"hello\");
+    println!(\"Hello, world!\");
 }
";
        let files = parse_patch(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/hello.rs");
        assert_eq!(files[0].hunks.len(), 1);
        assert_eq!(files[0].hunks[0].old_start, 1);
        assert_eq!(files[0].hunks[0].old_count, 3);
        assert_eq!(files[0].hunks[0].new_count, 3);
        assert_eq!(files[0].hunks[0].lines.len(), 4);
        assert_eq!(
            files[0].hunks[0].lines[0],
            PatchLine::Context("fn main() {".into())
        );
        assert_eq!(
            files[0].hunks[0].lines[1],
            PatchLine::Remove("    println!(\"hello\");".into())
        );
        assert_eq!(
            files[0].hunks[0].lines[2],
            PatchLine::Add("    println!(\"Hello, world!\");".into())
        );
        assert_eq!(files[0].hunks[0].lines[3], PatchLine::Context("}".into()));
    }
}

mod edge_cases {
    use super::*;

    #[test]
    fn apply_hunks_fuzz_match() {
        // The hunk header says line 2, but the actual match is at line 3
        // (1 line off). Should still apply within FUZZ_RANGE=3.
        let original = "a\nb\nc\nd\n";
        let hunks = vec![Hunk {
            old_start: 2,
            old_count: 1,
            new_start: 2,
            new_count: 1,
            lines: vec![PatchLine::Remove("c".into()), PatchLine::Add("C".into())],
        }];
        let result = apply_hunks(original, &hunks).unwrap();
        assert_eq!(result, "a\nb\nC\nd\n");
    }

    #[test]
    fn apply_hunks_pure_addition_on_empty() {
        // A patch that creates a file from scratch: old_start=0, old_count=0,
        // hunk contains only additions.
        let original = "";
        let hunks = vec![Hunk {
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: 2,
            lines: vec![
                PatchLine::Add("new_line1".into()),
                PatchLine::Add("new_line2".into()),
            ],
        }];
        let result = apply_hunks(original, &hunks).unwrap();
        // Empty original is treated as having a final newline, so the
        // output also gets one.
        assert_eq!(result, "new_line1\nnew_line2\n");
    }

    #[test]
    fn merge_three_way_lines_all_unchanged() {
        // all three identical → no change, no conflict
        let base = s(&["A", "B"]);
        let ours = s(&["A", "B"]);
        let theirs = s(&["A", "B"]);
        let (result, conflicts) = merge_three_way_lines(&base, &ours, &theirs, 1);
        assert_eq!(result, s(&["A", "B"]));
        assert!(conflicts.is_empty());
    }

    #[test]
    fn merge_three_way_block_ours_equals_theirs() {
        // ours == theirs, both differ from base → take ours, no conflict
        let base = s(&["old"]);
        let ours = s(&["new1", "new2"]);
        let theirs = s(&["new1", "new2"]);
        let (result, conflicts) = merge_three_way_block(&base, &ours, &theirs, 1);
        assert_eq!(result, s(&["new1", "new2"]));
        assert!(conflicts.is_empty());
    }

    #[test]
    fn find_match_global_empty_needle() {
        let haystack: Vec<&str> = vec!["A", "B"];
        let needle: Vec<&str> = vec![];
        assert_eq!(find_match_global(&haystack, &needle), Some(0));
    }

    #[test]
    fn find_match_global_no_match() {
        let haystack: Vec<&str> = vec!["A", "B", "C"];
        let needle: Vec<&str> = vec!["X", "Y"];
        assert_eq!(find_match_global(&haystack, &needle), None);
    }

    // Regression: needle longer than haystack caused a panic because
    // saturating_sub produced 0 as max_start, then the loop tried to
    // slice haystack[0..needle.len()] which was out of bounds.
    #[test]
    fn find_match_global_needle_longer_than_haystack() {
        let haystack: Vec<&str> = vec![];
        let needle: Vec<&str> = vec!["A", "B", "C"];
        assert_eq!(find_match_global(&haystack, &needle), None);

        let short_haystack: Vec<&str> = vec!["A"];
        assert_eq!(find_match_global(&short_haystack, &needle), None);
    }

    #[test]
    fn locate_by_context_anchors_no_context_returns_none() {
        // Hunk has no context lines at all → cannot locate
        let hunk = Hunk {
            old_start: 1,
            old_count: 1,
            new_start: 1,
            new_count: 1,
            lines: vec![
                PatchLine::Remove("old".into()),
                PatchLine::Add("new".into()),
            ],
        };
        let haystack: Vec<&str> = vec!["modified"];
        let result = locate_by_context_anchors(&haystack, &hunk, 0);
        assert_eq!(result, None);
    }

    #[test]
    fn fuzz_at_maximum_boundary_delta_3() {
        // "target" is at 0-indexed line 4 (line 5 in 1-indexed).
        // Hunk says old_start=2, so expected = 2-1 = 1 (0-indexed).
        // Delta needed: |4 - 1| = 3, exactly FUZZ_RANGE. Should match.
        let original = "a\nb\nc\nd\ntarget\nf\n";
        let hunks = vec![Hunk {
            old_start: 2,
            old_count: 1,
            new_start: 2,
            new_count: 1,
            lines: vec![
                PatchLine::Remove("target".into()),
                PatchLine::Add("TARGET".into()),
            ],
        }];
        let result = apply_hunks(original, &hunks).unwrap();
        assert_eq!(result, "a\nb\nc\nd\nTARGET\nf\n");
    }

    #[test]
    fn fuzz_beyond_boundary_delta_4_fails() {
        // "target" is at 0-indexed line 5 (line 6 in 1-indexed).
        // Hunk says old_start=2, so expected = 2-1 = 1 (0-indexed).
        // Delta needed: |5 - 1| = 4, beyond FUZZ_RANGE=3. Should fail.
        let original = "a\nb\nc\nd\ne\ntarget\nf\n";
        let hunks = vec![Hunk {
            old_start: 2,
            old_count: 1,
            new_start: 2,
            new_count: 1,
            lines: vec![
                PatchLine::Remove("target".into()),
                PatchLine::Add("TARGET".into()),
            ],
        }];
        assert!(apply_hunks(original, &hunks).is_err());
    }

    #[test]
    fn two_hunks_both_requiring_fuzz() {
        // Line layout (0-indexed): 0=a, 1=b, 2=x, 3=c, 4=d, 5=y, 6=e
        // Hunk 1: old_start=1, expected=0, "x" is at index 2. Delta=2 (within fuzz).
        // Hunk 2: old_start=4, expected=3+offset(0)=3, "y" is at index 5. Delta=2.
        let original = "a\nb\nx\nc\nd\ny\ne\n";
        let hunks = vec![
            Hunk {
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 1,
                lines: vec![PatchLine::Remove("x".into()), PatchLine::Add("X".into())],
            },
            Hunk {
                old_start: 4,
                old_count: 1,
                new_start: 4,
                new_count: 1,
                lines: vec![PatchLine::Remove("y".into()), PatchLine::Add("Y".into())],
            },
        ];
        let result = apply_hunks(original, &hunks).unwrap();
        assert_eq!(result, "a\nb\nX\nc\nd\nY\ne\n");
    }

    #[test]
    fn hunk_removes_all_lines() {
        let original = "a\nb\nc\n";
        let hunks = vec![Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 0,
            lines: vec![
                PatchLine::Remove("a".into()),
                PatchLine::Remove("b".into()),
                PatchLine::Remove("c".into()),
            ],
        }];
        let result = apply_hunks(original, &hunks).unwrap();
        // All lines removed; join_lines on empty vec returns "".
        assert_eq!(result, "");
    }

    #[test]
    fn context_only_hunk_is_noop() {
        // All lines are Context, no Add or Remove. Content should not change.
        let original = "line1\nline2\nline3\n";
        let hunks = vec![Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                PatchLine::Context("line1".into()),
                PatchLine::Context("line2".into()),
                PatchLine::Context("line3".into()),
            ],
        }];
        let result = apply_hunks(original, &hunks).unwrap();
        assert_eq!(result, original);
    }
}

mod error_handling {
    use super::*;

    #[test]
    fn parse_patch_no_files() {
        let diff = "just some text\n";
        assert!(parse_patch(diff).is_err());
    }

    #[test]
    fn parse_patch_no_hunks() {
        let diff = "--- a/f.txt\n+++ b/f.txt\n";
        assert!(parse_patch(diff).is_err());
    }

    #[test]
    fn apply_hunks_stale_context_fails() {
        let original = "a\nb\nc\n";
        let hunks = vec![Hunk {
            old_start: 1,
            old_count: 1,
            new_start: 1,
            new_count: 1,
            lines: vec![
                PatchLine::Remove("wrong_context".into()),
                PatchLine::Add("x".into()),
            ],
        }];
        assert!(apply_hunks(original, &hunks).is_err());
    }

    #[test]
    fn apply_with_options_merge_conflict_disallowed() {
        // Stale content, conflicting merge, allow_conflicts = false → Err
        let ours = "ctx1\nours_mod\nctx2\n";
        let hunks = vec![make_hunk(1, &["ctx1"], &["base"], &["theirs"], &["ctx2"])];
        let opts = ApplyHunksOptions {
            on_stale: OnStale::Merge,
            allow_conflicts: false,
        };
        let result = apply_hunks_with_options(ours, &hunks, opts);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("conflict"));
    }

    #[test]
    fn apply_with_options_fail_on_stale() {
        // OnStale::Fail with stale content → Err
        let ours = "ctx1\nmodified\nctx2\n";
        let hunks = vec![make_hunk(1, &["ctx1"], &["original"], &["new"], &["ctx2"])];
        let opts = ApplyHunksOptions {
            on_stale: OnStale::Fail,
            allow_conflicts: false,
        };
        let result = apply_hunks_with_options(ours, &hunks, opts);
        assert!(result.is_err());
    }
}

mod format_preservation {
    use super::*;

    #[test]
    fn apply_hunks_preserves_no_final_newline() {
        let original = "line1\nline2";
        let hunks = vec![Hunk {
            old_start: 2,
            old_count: 1,
            new_start: 2,
            new_count: 1,
            lines: vec![
                PatchLine::Remove("line2".into()),
                PatchLine::Add("LINE2".into()),
            ],
        }];
        let result = apply_hunks(original, &hunks).unwrap();
        assert_eq!(result, "line1\nLINE2");
    }

    #[test]
    fn merge_hunks_preserves_final_newline() {
        let ours = "ctx1\nold\nctx2\n";
        let hunks = vec![make_hunk(1, &["ctx1"], &["old"], &["new"], &["ctx2"])];
        let result = merge_hunks(ours, &hunks).unwrap();
        assert!(result.content.ends_with('\n'));
    }

    #[test]
    fn merge_hunks_no_final_newline() {
        let ours = "ctx1\nold\nctx2";
        let hunks = vec![make_hunk(1, &["ctx1"], &["old"], &["new"], &["ctx2"])];
        let result = merge_hunks(ours, &hunks).unwrap();
        assert!(!result.content.ends_with('\n'));
        assert_eq!(result.content, "ctx1\nnew\nctx2");
    }

    #[test]
    fn parse_patch_skips_no_newline_marker() {
        // The "\ No newline at end of file" marker must be silently skipped.
        let diff = "\
--- a/file.txt
+++ b/file.txt
@@ -1,2 +1,2 @@
 keep
-old
+new
\\ No newline at end of file
";
        let files = parse_patch(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].hunks[0].lines.len(), 3);
        // Only Context("keep"), Remove("old"), Add("new") -- marker not present
        assert_eq!(
            files[0].hunks[0].lines[0],
            PatchLine::Context("keep".into())
        );
        assert_eq!(files[0].hunks[0].lines[1], PatchLine::Remove("old".into()));
        assert_eq!(files[0].hunks[0].lines[2], PatchLine::Add("new".into()));
    }
}

mod regression {
    use super::*;

    /// Regression: apply_hunks must not panic on huge old_start values
    /// that would overflow isize when cast from usize. Found by fuzzing.
    #[test]
    fn apply_hunks_huge_old_start_does_not_panic() {
        let hunks = vec![Hunk {
            old_start: usize::MAX,
            old_count: 1,
            new_start: 1,
            new_count: 1,
            lines: vec![PatchLine::Context("x".into())],
        }];
        // Must return Err, never panic.
        assert!(apply_hunks("x\n", &hunks).is_err());
    }

    /// Regression: merge_hunks must produce correct ConflictRange line
    /// numbers for multi-hunk patches. The old code double-counted hunk
    /// sizes via an `output_line` accumulator on top of the `pos` index
    /// which already reflected previous splices.
    #[test]
    fn merge_hunks_multi_hunk_conflict_lines_are_correct() {
        // Hunk 1 at line 2: change "line2" to "changed2" (base says "line2").
        // Hunk 2 at line 8: change "line8" to "changed8" (base says "line8").
        let hunks = vec![
            make_hunk(1, &["line1"], &["line2"], &["changed2"], &["line3"]),
            make_hunk(7, &["line7"], &["line8"], &["changed8"], &["line9"]),
        ];
        // Simulate "ours" already having different content at hunk 2 to force a conflict.
        let ours = "line1\nline2\nline3\nline4\nline5\nline6\nline7\nours8\nline9\nline10\n";
        let result = merge_hunks(ours, &hunks).unwrap();
        // Hunk 1 applies cleanly (ours matches base for hunk 1).
        // Hunk 2 conflicts because ours has "ours8" while base has "line8".
        assert_eq!(
            result.conflicts.len(),
            1,
            "should have exactly one conflict"
        );
        let conflict = &result.conflicts[0];
        // The conflict should be at the position of hunk 2 in the output.
        // Find the conflict marker in the actual output to verify.
        let lines: Vec<&str> = result.content.lines().collect();
        let marker_line = lines
            .iter()
            .position(|l| l.contains("<<<<<<< patchloom"))
            .expect("conflict marker should exist");
        // ConflictRange is 1-based.
        assert_eq!(
            conflict.start_line,
            marker_line + 1,
            "conflict start_line should match actual marker position"
        );
    }

    /// Regression: find_match must not panic when delta * sign overflows.
    /// Found by fuzzing.
    #[test]
    fn apply_hunks_huge_fuzz_range_does_not_panic() {
        let hunks = vec![Hunk {
            old_start: 1,
            old_count: 0,
            new_start: 1,
            new_count: 1,
            lines: vec![PatchLine::Add("new".into())],
        }];
        // apply_hunks uses a fuzz of 2 internally; the regression was
        // in find_match when delta values caused isize overflow. Just
        // verify it doesn't panic.
        let _ = apply_hunks("original\n", &hunks);
    }

    #[test]
    fn parse_file_path_strips_tab_timestamp() {
        let result = parse_file_path("+++ b/file.txt\t2024-01-01 00:00:01.000000000 +0000");
        assert_eq!(result, "file.txt");
    }

    #[test]
    fn parse_file_path_no_tab_unchanged() {
        let result = parse_file_path("+++ b/file.txt");
        assert_eq!(result, "file.txt");
    }

    #[test]
    fn parse_file_path_minus_with_tab_timestamp() {
        let result = parse_file_path("--- a/src/main.rs\t2024-06-01 12:00:00.000 +0000");
        assert_eq!(result, "src/main.rs");
    }

    #[test]
    fn parse_patches_deletion_uses_minus_path() {
        // Regression: deletion patches have `+++ /dev/null` so the path must
        // come from the `---` line.  Previously the parser always read from
        // `+++`, producing "/dev/null" as the file path.
        let diff = "\
--- a/old_file.txt
+++ /dev/null
@@ -1,2 +0,0 @@
-line one
-line two
";
        let patches = parse_patch(diff).expect("should parse deletion patch");
        assert_eq!(patches.len(), 1);
        assert_eq!(
            patches[0].path, "old_file.txt",
            "path should come from --- line for deletions"
        );
    }

    #[test]
    fn hunk_context_anchors_mid_hunk_context_excluded_from_suffix() {
        // Regression: mid-hunk context between two change blocks was included
        // in the suffix, degrading merge fallback accuracy.
        let hunk = Hunk {
            old_start: 1,
            old_count: 5,
            new_start: 1,
            new_count: 5,
            lines: vec![
                PatchLine::Context("before".into()),
                PatchLine::Remove("old1".into()),
                PatchLine::Add("new1".into()),
                PatchLine::Context("mid".into()),
                PatchLine::Remove("old2".into()),
                PatchLine::Add("new2".into()),
                PatchLine::Context("after".into()),
            ],
        };
        let (prefix, suffix) = hunk_context_anchors(&hunk);
        assert_eq!(
            prefix,
            vec!["before"],
            "prefix should be leading context only"
        );
        assert_eq!(
            suffix,
            vec!["after"],
            "suffix should be trailing context only, not mid-hunk context"
        );
    }

    #[test]
    fn apply_hunks_preserves_crlf() {
        // Regression: apply_hunks converted CRLF to LF by splitting on
        // .lines() and rejoining with \n.
        let original = "line1\r\nline2\r\nline3\r\n";
        let hunk = Hunk {
            old_start: 2,
            old_count: 1,
            new_start: 2,
            new_count: 1,
            lines: vec![
                PatchLine::Remove("line2".into()),
                PatchLine::Add("replaced".into()),
            ],
        };
        let result = apply_hunks(original, &[hunk]).expect("should apply");
        assert!(
            result.contains("\r\n"),
            "CRLF should be preserved, got: {:?}",
            result
        );
        assert_eq!(result, "line1\r\nreplaced\r\nline3\r\n");
    }
}
