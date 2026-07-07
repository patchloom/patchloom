use super::*;

#[test]
fn test_md_replace_section() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(
        &file,
        "# Title\n\n## Section\n\nold content\n\n## Other\n\nkept\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("replace-section")
        .arg(&file)
        .arg("--heading")
        .arg("## Section")
        .arg("--content")
        .arg("new content")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("new content"),
        "should contain new content"
    );
    assert!(
        !content.contains("old content"),
        "should not contain old content"
    );
    assert!(
        content.contains("## Other"),
        "Other section heading should be intact"
    );
    assert!(
        content.contains("kept"),
        "Other section content should be intact"
    );
}

#[test]
fn test_md_default_mode_shows_diff() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(
        &file,
        "# Title\n\n## Section\n\nold content\n\n## Other\n\nkept\n",
    )
    .unwrap();

    // Default mode (no --apply/--check/--diff) should show a unified diff preview.
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("replace-section")
        .arg(&file)
        .arg("--heading")
        .arg("## Section")
        .arg("--content")
        .arg("new content")
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "default mode with changes should exit 2 (CHANGES_DETECTED)"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--- a/"),
        "default mode should show diff header: {stdout}"
    );
    assert!(
        stdout.contains("-old content"),
        "default mode should show removed line: {stdout}"
    );
    assert!(
        stdout.contains("+new content"),
        "default mode should show added line: {stdout}"
    );

    // File should NOT be modified in default mode.
    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("old content"),
        "default mode should not modify the file"
    );
}

#[test]
fn test_md_table_append() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(&file, "## Table\n\n| H1 | H2 |\n|---|---|\n| A | B |\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("table-append")
        .arg(&file)
        .arg("--heading")
        .arg("## Table")
        .arg("--row")
        .arg("| new | row |")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("| new | row |"),
        "new row should be present"
    );
    assert!(
        content.contains("| A | B |"),
        "existing row should be preserved"
    );
}

#[test]
fn test_md_insert_after_heading() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("readme.md");
    fs::write(&file, "# Title\n\nExisting content\n\n## Other\n\nMore\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("insert-after-heading")
        .arg(&file)
        .arg("--heading")
        .arg("# Title")
        .arg("--content")
        .arg("Inserted line")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("Inserted line"),
        "inserted content should appear"
    );
    assert!(
        content.contains("Existing content"),
        "existing content should be preserved"
    );
}

#[test]
fn test_md_upsert_bullet_adds_new() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("notes.md");
    fs::write(&file, "## Rules\n\n- existing rule\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("upsert-bullet")
        .arg(&file)
        .arg("--heading")
        .arg("## Rules")
        .arg("--bullet")
        .arg("new rule")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("new rule"), "new bullet should be added");
    assert!(
        content.contains("- existing rule"),
        "existing bullet should be preserved"
    );
}

#[test]
fn test_md_upsert_bullet_skips_duplicate() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("notes.md");
    fs::write(&file, "## Rules\n\n- existing rule\n").unwrap();

    // Upsert the same bullet; should be idempotent.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("upsert-bullet")
        .arg(&file)
        .arg("--heading")
        .arg("## Rules")
        .arg("--bullet")
        .arg("existing rule")
        .arg("--check")
        .assert()
        .code(0); // no changes -> exit 0
}

// ---------------------------------------------------------------------------
// tidy
// ---------------------------------------------------------------------------

#[test]
fn test_md_dedupe_headings_removes_duplicate() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(&file, "# Title\n\n## Dup\n\nFirst\n\n## Dup\n\nSecond\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("dedupe-headings")
        .arg(&file)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let count = content.matches("## Dup").count();
    assert_eq!(count, 1, "duplicate heading should be removed");
}

#[test]
fn test_md_dedupe_headings_default_mode_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(&file, "# Title\n\n## Dup\n\nFirst\n\n## Dup\n\nSecond\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("dedupe-headings")
        .arg(&file)
        .assert()
        .code(2);

    // File should not be modified in preview mode
    let content = fs::read_to_string(&file).unwrap();
    let count = content.matches("## Dup").count();
    assert_eq!(count, 2, "preview mode should not modify file");
}

#[test]
fn test_md_dedupe_headings_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(&file, "# Title\n\n## Dup\n\nFirst\n\n## Dup\n\nSecond\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("md")
        .arg("dedupe-headings")
        .arg(&file)
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1);
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json.as_str().unwrap(), "## Dup");
}

#[test]
fn test_md_dedupe_headings_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(&file, "# Title\n\n## Dup\n\nFirst\n\n## Dup\n\nSecond\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("md")
        .arg("dedupe-headings")
        .arg(&file)
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let arr = parsed.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0].as_str().unwrap(), "## Dup");
}

#[test]
fn test_md_lint_agents_quiet_suppresses_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    fs::write(&file, "# T\n\nUse git add .\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("md")
        .arg("lint-agents")
        .arg(&file)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stdout.is_empty(),
        "quiet should suppress lint-agents output"
    );
}

#[test]
fn test_md_lint_agents_clean_file_exits_0() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    fs::write(&file, "# AGENTS.md\n\n## Build\n\nRun make\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("lint-agents")
        .arg(&file)
        .assert()
        .code(0);
}

#[test]
fn test_md_lint_agents_bad_file_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    fs::write(
        &file,
        "# AGENTS.md\n\n## Build\n\nRun make\n\n## Build\n\nDuplicate\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("lint-agents")
        .arg(&file)
        .assert()
        .code(2);
}

#[test]
fn test_md_lint_agents_skips_fenced_code_blocks() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    // The dangerous command appears inside both backtick and tilde fences —
    // lint-agents must not flag it.
    fs::write(
        &file,
        "# Rules\n\n```bash\ngit add .\n```\n\n~~~bash\ngit add -A\n~~~\n\nStage explicitly.\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("lint-agents")
        .arg(&file)
        .assert()
        .code(0);
}

#[test]
fn test_md_lint_agents_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    fs::write(
        &file,
        "# AGENTS.md\n\n## Build\n\nRun make\n\n## Build\n\nDuplicate\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("md")
        .arg("lint-agents")
        .arg(&file)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let arr = parsed.as_array().unwrap();
    assert!(!arr.is_empty());
    // Each issue must have an "issue" string and a "heading" identifying it
    let issue_val = arr[0].get("issue").expect("issue field missing");
    let issue_str = issue_val.as_str().expect("issue field should be a string");
    assert!(
        issue_str.contains("duplicate"),
        "should report a duplicate heading issue: {issue_str}"
    );
    let heading_val = arr[0].get("heading").expect("heading field missing");
    assert!(
        heading_val.as_str().unwrap().contains("Build"),
        "heading should reference 'Build': {heading_val}"
    );
}

#[test]
fn test_md_lint_agents_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    fs::write(
        &file,
        "# AGENTS.md\n\n## Build\n\nRun make\n\n## Build\n\nDuplicate\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("md")
        .arg("lint-agents")
        .arg(&file)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert!(!lines.is_empty());
    // Each line must be valid JSON with an "issue" string field
    for line in &lines {
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        let issue_val = parsed
            .get("issue")
            .expect("issue field missing in JSONL line");
        assert!(
            issue_val.is_string(),
            "issue field should be a string: {issue_val}"
        );
    }
}

// ---------------------------------------------------------------------------
// md move-section (CLI)
// ---------------------------------------------------------------------------

#[test]
fn test_md_move_section_equivalent_path_reorder() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(&file, "# A\na-content\n# B\nb-content\n# C\nc-content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("move-section")
        .arg(&file)
        .arg("--heading")
        .arg("C")
        .arg("--before")
        .arg("B")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let a_pos = content.find("# A").unwrap();
    let c_pos = content.find("# C").unwrap();
    let b_pos = content.find("# B").unwrap();
    assert!(a_pos < c_pos, "A should come before C");
    assert!(c_pos < b_pos, "C should come before B after move");
    assert!(content.contains("a-content"));
    assert!(content.contains("b-content"));
    assert!(content.contains("c-content"));
}

#[test]
fn test_md_move_section_explicit_to_equivalent_path_reorders() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(&file, "# A\na-content\n# B\nb-content\n# C\nc-content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("move-section")
        .arg(&file)
        .arg("--heading")
        .arg("C")
        .arg("--to")
        .arg(&file)
        .arg("--before")
        .arg("B")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let a_pos = content.find("# A").unwrap();
    let c_pos = content.find("# C").unwrap();
    let b_pos = content.find("# B").unwrap();
    assert!(c_pos < b_pos, "C should be before B after move");
    assert!(a_pos < c_pos, "A should still be first");
    assert_eq!(
        content.matches("# C").count(),
        1,
        "section C must not be duplicated"
    );
}

#[test]
fn test_md_move_section_cross_file() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("source.md");
    let dst = dir.path().join("dest.md");
    fs::write(&src, "# Keep\nkept\n# Move\nmoved\n# Stay\nstayed\n").unwrap();
    fs::write(&dst, "# Intro\nintro\n# End\nend\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("move-section")
        .arg(&src)
        .arg("--heading")
        .arg("Move")
        .arg("--to")
        .arg(&dst)
        .arg("--before")
        .arg("End")
        .arg("--apply")
        .assert()
        .code(0);

    let src_content = fs::read_to_string(&src).unwrap();
    assert!(
        !src_content.contains("# Move"),
        "section should be removed from source"
    );
    assert!(
        !src_content.contains("moved"),
        "section body should be removed from source"
    );
    assert!(
        src_content.contains("# Keep"),
        "other sections should be preserved in source"
    );
    assert!(
        src_content.contains("# Stay"),
        "other sections should be preserved in source"
    );

    let dst_content = fs::read_to_string(&dst).unwrap();
    assert!(
        dst_content.contains("# Move"),
        "section should appear in dest"
    );
    assert!(
        dst_content.contains("moved"),
        "section body should appear in dest"
    );
    let move_pos = dst_content.find("# Move").unwrap();
    let end_pos = dst_content.find("# End").unwrap();
    assert!(move_pos < end_pos, "moved section should be before # End");
}

#[test]
fn test_md_move_section_missing_heading_exits_3() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(&file, "# A\ncontent\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("move-section")
        .arg(&file)
        .arg("--heading")
        .arg("Missing")
        .arg("--before")
        .arg("A")
        .arg("--apply")
        .assert()
        .code(3);
}

#[test]
fn test_md_move_section_check_mode_exits_2_no_write() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    let original = "# A\na-content\n# B\nb-content\n# C\nc-content\n";
    fs::write(&file, original).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("move-section")
        .arg(&file)
        .arg("--heading")
        .arg("C")
        .arg("--before")
        .arg("B")
        .arg("--check")
        .assert()
        .code(2);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, original,
        "file should be unchanged in --check mode"
    );
}

#[test]
fn test_md_move_section_cross_file_check_mode_reports_both_files() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("source.md");
    let dst = dir.path().join("dest.md");
    let src_original = "# Keep\nkept\n# Move\nmoved\n";
    let dst_original = "# Intro\nintro\n# End\nend\n";
    fs::write(&src, src_original).unwrap();
    fs::write(&dst, dst_original).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("move-section")
        .arg(&src)
        .arg("--heading")
        .arg("Move")
        .arg("--to")
        .arg(&dst)
        .arg("--before")
        .arg("End")
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));

    // Both files should be unchanged.
    let src_content = fs::read_to_string(&src).unwrap();
    assert_eq!(
        src_content, src_original,
        "source should be unchanged in --check mode"
    );
    let dst_content = fs::read_to_string(&dst).unwrap();
    assert_eq!(
        dst_content, dst_original,
        "dest should be unchanged in --check mode"
    );

    // Both files should be mentioned in the output.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("source.md"),
        "source file should be reported in --check output: {stdout}"
    );
    assert!(
        stdout.contains("dest.md"),
        "dest file should be reported in --check output: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// global flags: --jsonl, --files-from, --context, --literal
// ---------------------------------------------------------------------------

#[test]
fn test_md_replace_section_check_exits_2_no_write() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    let original = "# Title\n\n## Section\n\nold content\n\n## Other\n\nkept\n";
    fs::write(&file, original).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("replace-section")
        .arg(&file)
        .arg("--heading")
        .arg("## Section")
        .arg("--content")
        .arg("new content")
        .arg("--check")
        .assert()
        .code(2);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, original,
        "file should be unchanged in --check mode"
    );
}

// ---------------------------------------------------------------------------
// tidy fix --check
// ---------------------------------------------------------------------------

#[test]
fn test_md_apply_check_does_not_write() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    let original = "# Title\n\n## Section\n\nold content\n";
    fs::write(&file, original).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("replace-section")
        .arg(&file)
        .arg("--heading")
        .arg("## Section")
        .arg("--content")
        .arg("new content")
        .arg("--apply")
        .arg("--check")
        .assert()
        .code(2);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, original,
        "--check should prevent writing even with --apply"
    );
}

// ---------------------------------------------------------------------------
// tx: glob-based replace
// ---------------------------------------------------------------------------

#[test]
fn test_md_insert_before_heading() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(
        &file,
        "# Title\n\nIntro.\n\n## Section A\n\nContent A.\n\n## Section B\n\nContent B.\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("insert-before-heading")
        .arg(file.to_str().unwrap())
        .arg("--heading")
        .arg("Section B")
        .arg("--content")
        .arg("Inserted before B.")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("Inserted before B.\n\n## Section B"));
}

#[test]
fn test_md_table_append_missing_file_includes_path_in_error() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("table-append")
        .arg("does-not-exist.md")
        .arg("--heading")
        .arg("## T")
        .arg("--row")
        .arg("| a | b |")
        .arg("--apply")
        .assert()
        .code(1)
        .stderr(predicates::str::contains("does-not-exist.md"));
}

#[test]
fn test_md_insert_before_heading_not_found() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# Title\n\nContent.\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("insert-before-heading")
        .arg(file.to_str().unwrap())
        .arg("--heading")
        .arg("Nonexistent")
        .arg("--content")
        .arg("text")
        .arg("--apply")
        .assert()
        .code(3); // NO_MATCHES
}

#[test]
fn test_md_insert_after_heading_not_found() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# Title\n\nContent.\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("insert-after-heading")
        .arg(file.to_str().unwrap())
        .arg("--heading")
        .arg("Nonexistent")
        .arg("--content")
        .arg("text")
        .arg("--apply")
        .assert()
        .code(3); // NO_MATCHES
}

#[test]
fn test_md_upsert_bullet_heading_not_found() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# Title\n\nContent.\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("upsert-bullet")
        .arg(file.to_str().unwrap())
        .arg("--heading")
        .arg("Nonexistent")
        .arg("--bullet")
        .arg("new item")
        .arg("--apply")
        .assert()
        .code(3); // NO_MATCHES
}

#[test]
fn test_md_insert_before_heading_honors_cwd() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "# Title\n\n## Section A\n\nBody A\n\n## Section B\n\nBody B\n",
    )
    .unwrap();

    patchloom_in(dir.path())
        .arg("md")
        .arg("insert-before-heading")
        .arg("doc.md")
        .arg("--heading")
        .arg("Section B")
        .arg("--content")
        .arg("Inserted before B.")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(dir.path().join("doc.md")).unwrap();
    assert!(content.contains("Inserted before B.\n\n## Section B"));
}

#[test]
fn test_md_check_produces_stdout() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(
        &file,
        "# Title\n\n## Section\n\nold content\n\n## Other\n\nkept\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("replace-section")
        .arg(&file)
        .arg("--heading")
        .arg("## Section")
        .arg("--content")
        .arg("new content")
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("would modify"),
        "md --check should produce stdout, got: {stdout}"
    );
}

#[test]
fn test_md_check_json_produces_structured_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(
        &file,
        "# Title\n\n## Section\n\nold content\n\n## Other\n\nkept\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("md")
        .arg("replace-section")
        .arg(&file)
        .arg("--heading")
        .arg("## Section")
        .arg("--content")
        .arg("new content")
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|_| panic!("expected JSON output, got: {stdout}"));
    assert_eq!(json["ok"], true, "check output should have ok=true");
    assert_eq!(
        json["has_changes"], true,
        "check output should have has_changes=true"
    );
}

// ---------------------------------------------------------------------------
// table-append column count validation (#1172)
// ---------------------------------------------------------------------------

/// Table append with nonexistent heading returns exit 3 (#1331 follow-up).
#[test]
fn test_md_table_append_nonexistent_heading_exits_3() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("t.md");
    fs::write(
        &file,
        "# API\n| Name | Value |\n|------|-------|\n| a | 1 |\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "md",
            "table-append",
            file.to_str().unwrap(),
            "--heading",
            "Nonexistent",
            "--row",
            "| b | 2 |",
            "--apply",
        ])
        .assert()
        .code(3);
}

/// Table append with wrong column count should fail (#1172).
#[test]
fn test_md_table_append_wrong_column_count_fails() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("t.md");
    fs::write(
        &file,
        "# API\n| Name | Value | Status |\n|------|-------|--------|\n| a | 1 | ok |\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "md",
            "table-append",
            file.to_str().unwrap(),
            "--heading",
            "API",
            "--row",
            "| b |",
            "--apply",
        ])
        .assert()
        .code(1);

    // File should be unchanged.
    let content = fs::read_to_string(&file).unwrap();
    assert!(
        !content.contains("| b |"),
        "wrong-column row should NOT be appended"
    );
}

// ---------------------------------------------------------------------------
// upsert-bullet dedup only against actual bullets (#1173)
// ---------------------------------------------------------------------------

/// Paragraph text matching bullet content should not prevent insertion (#1173).
#[test]
fn test_md_upsert_bullet_does_not_dedup_against_paragraphs() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("rules.md");
    fs::write(&file, "# Rules\nRun make check\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["md", "upsert-bullet"])
        .arg(file.to_str().unwrap())
        .args(["--heading", "Rules", "--bullet"])
        .arg("* Run make check")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("* Run make check"),
        "bullet should be inserted even when paragraph has same text"
    );
}

// ---------------------------------------------------------------------------
// No-match text-mode stderr tests
// ---------------------------------------------------------------------------

#[test]
fn test_md_replace_section_no_match_emits_stderr_in_text_mode() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("readme.md");
    fs::write(&file, "# Intro\nHello\n").unwrap();

    patchloom_in(dir.path())
        .args([
            "md",
            "replace-section",
            file.to_str().unwrap(),
            "--heading",
            "## Nonexistent",
            "--content",
            "new content",
            "--apply",
        ])
        .assert()
        .code(3)
        .stderr(predicates::str::contains("md.replace_section"));
}

// ---------------------------------------------------------------------------
// doc --check produces stdout output (#544)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// --contain and agent aliases (MPI 2026-07-07 cycle 1 QA)
// ---------------------------------------------------------------------------

#[test]
fn test_md_replace_section_contain_rejects_parent_escape() {
    let dir = TempDir::new().unwrap();
    let escape_name = format!(
        "patchloom-md-escape-{}.md",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let outside = dir.path().parent().unwrap().join(&escape_name);
    fs::write(&outside, "# Title\n\n## Section\n\nold\n").unwrap();

    patchloom_in(dir.path())
        .args([
            "--contain",
            "md",
            "replace-section",
            &format!("../{escape_name}"),
            "--heading",
            "## Section",
            "--content",
            "injected",
            "--apply",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );

    let content = fs::read_to_string(&outside).unwrap();
    assert!(
        content.contains("old"),
        "escaped file must not be mutated under --contain"
    );
    let _ = fs::remove_file(&outside);
}

#[test]
fn test_md_lint_alias_invokes_lint_agents() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    // Missing trailing newline is a common lint-agents finding path; content
    // that is well-formed still must accept the `lint` alias without clap error.
    fs::write(&file, "# Rules\n\n- Prefer patchloom for edits\n").unwrap();

    patchloom_in(dir.path())
        .args(["md", "lint", "AGENTS.md"])
        .assert()
        .success();
}
