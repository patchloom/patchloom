use super::*;

#[test]
fn test_patch_apply_dry_run_does_not_modify_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nold line\nline3\n").unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    // Without --apply, patchloom patch apply should show diff but NOT write.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "line1\nold line\nline3\n",
        "file should be unchanged in dry-run mode"
    );
}

#[test]
fn test_patch_apply_with_apply_flag_writes_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nold line\nline3\n").unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "line1\nnew line\nline3\n");
}

#[test]
fn test_patch_apply_json_parse_error_returns_error_object() {
    let dir = TempDir::new().unwrap();
    let patch_file = dir.path().join("bad.patch");
    fs::write(&patch_file, "not a patch\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .output()
        .unwrap();

    assert_patch_apply_error_object(&output, 4, "patch: parse error:");
}

#[test]
fn test_patch_apply_json_stale_error_returns_error_object() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let patch_file = dir.path().join("stale.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1 +1 @@\n-old\n+new\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--apply")
        .output()
        .unwrap();

    assert_patch_apply_error_object(&output, 5, "patch apply: test.txt -- STALE:");
}

#[test]
fn test_patch_check_exits_0_when_clean() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nold line\nline3\n").unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .assert()
        .code(0);
}

#[test]
fn test_patch_apply_check_counts_only_changed_files() {
    let dir = TempDir::new().unwrap();
    // File with a real change.
    let changed = dir.path().join("changed.txt");
    fs::write(&changed, "line1\nold line\nline3\n").unwrap();
    // File where the patch is a no-op (replace "same" with "same").
    let same = dir.path().join("same.txt");
    fs::write(&same, "same\n").unwrap();

    let patch_file = dir.path().join("multi.patch");
    fs::write(
        &patch_file,
        "--- a/changed.txt\n\
         +++ b/changed.txt\n\
         @@ -1,3 +1,3 @@\n\
         \x20line1\n\
         -old line\n\
         +new line\n\
         \x20line3\n\
         --- a/same.txt\n\
         +++ b/same.txt\n\
         @@ -1 +1 @@\n\
         -same\n\
         +same\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--check")
        .assert()
        .code(2) // CHANGES_DETECTED
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("1 file(s) would change"),
        "should count only files with actual changes, got: {stdout}"
    );
}

#[test]
fn test_patch_check_json_output_clean() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nold line\nline3\n").unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .arg("--json")
        .output()
        .unwrap();

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert!(json["files"].is_array());
    assert_eq!(json["files"][0]["status"], "clean");
}

#[test]
fn test_patch_apply_jsonl_parse_error_returns_error_object() {
    let dir = TempDir::new().unwrap();
    let patch_file = dir.path().join("bad.patch");
    fs::write(&patch_file, "not a patch\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .output()
        .unwrap();

    assert_patch_apply_error_object(&output, 4, "patch: parse error:");
}

#[test]
fn test_patch_check_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nold line\nline3\n").unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .arg("--jsonl")
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("each line should be valid JSON"))
        .collect();

    assert_eq!(lines.len(), 1, "should have one JSONL line per patch file");
    assert_eq!(lines[0]["path"], "test.txt");
    assert_eq!(lines[0]["status"], "clean");
}

#[test]
fn test_patch_check_exits_5_when_stale() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\ncompletely different\nline3\n").unwrap();

    let patch_file = dir.path().join("stale.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .assert()
        .code(5);
}

#[test]
fn test_patch_check_stale_human_readable_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\ncompletely different\nline3\n").unwrap();

    let patch_file = dir.path().join("stale.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .assert()
        .code(5)
        .stderr(predicates::str::contains("STALE"));
}

#[test]
fn test_patch_merge_check_exits_8_on_conflict() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\ncompletely different\nline3\n").unwrap();
    let patch_file = dir.path().join("stale.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("merge")
        .arg(&patch_file)
        .arg("--check")
        .assert()
        .code(8);
}

#[test]
fn test_patch_merge_check_allow_conflicts_exits_0() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\ncompletely different\nline3\n").unwrap();
    let patch_file = dir.path().join("stale.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();
    // With --allow-conflicts, --check should predict that apply would succeed
    // (conflicts are acceptable), so exit 0 instead of 8.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("merge")
        .arg(&patch_file)
        .arg("--check")
        .arg("--allow-conflicts")
        .assert()
        .code(0);
}

#[test]
fn test_patch_merge_allow_conflicts_writes_markers() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\ncompletely different\nline3\n").unwrap();
    let patch_file = dir.path().join("stale.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("merge")
        .arg(&patch_file)
        .arg("--apply")
        .arg("--allow-conflicts")
        .assert()
        .code(0);
    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("<<<<<<< patchloom (ours)"),
        "should have conflict markers: {content}"
    );
    assert!(
        content.contains("======="),
        "should have separator: {content}"
    );
    assert!(
        content.contains(">>>>>>> patch (theirs)"),
        "should have theirs marker: {content}"
    );
}

#[test]
fn test_patch_merge_apply_writes_clean_merge() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nold line\nline3\nextra\n").unwrap();
    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("merge")
        .arg(&patch_file)
        .arg("--apply")
        .assert()
        .code(0);
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "line1\nnew line\nline3\nextra\n"
    );
}

#[test]
fn test_patch_apply_on_stale_merge() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nold line\nline3\nextra\n").unwrap();
    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--on-stale")
        .arg("merge")
        .arg("--apply")
        .assert()
        .code(0);
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "line1\nnew line\nline3\nextra\n"
    );
}

#[test]
fn test_patch_check_exits_5_on_directory_read_error() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("test.txt");
    fs::create_dir(&target).unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -0,0 +1 @@\n+new line\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .assert()
        .code(5)
        .stderr(predicate::str::contains(
            "patch check: test.txt -- READ ERROR: failed to read",
        ));
}

#[test]
fn test_patch_check_json_reports_directory_read_error() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("test.txt");
    fs::create_dir(&target).unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -0,0 +1 @@\n+new line\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .arg("--json")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(5));

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["files"][0]["status"], "error");
    assert!(
        json["files"][0]["error"]
            .as_str()
            .unwrap()
            .contains("failed to read")
    );
}

#[test]
fn test_patch_check_jsonl_reports_directory_read_error() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("test.txt");
    fs::create_dir(&target).unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -0,0 +1 @@\n+new line\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .arg("--jsonl")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("each line should be valid JSON"))
        .collect();

    assert_eq!(lines.len(), 1, "should have one JSONL line per patch file");
    assert_eq!(lines[0]["path"], "test.txt");
    assert_eq!(lines[0]["status"], "error");
    assert!(
        lines[0]["error"]
            .as_str()
            .unwrap()
            .contains("failed to read")
    );
}

#[test]
fn test_patch_apply_check_quiet_suppresses_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nold line\nline3\n").unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--check")
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty());

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "line1\nold line\nline3\n"
    );
}

#[test]
fn test_patch_help_examples_use_subcommand() {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["patch", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("patchloom patch apply") || stdout.contains("patchloom patch check"),
        "patch help examples must use a subcommand (apply/check), got:\n{stdout}"
    );
    assert!(
        !stdout.contains("patchloom patch changes.patch"),
        "patch help examples must not show bare file arguments without a subcommand"
    );
}

// ---------------------------------------------------------------------------
// Parse smoke tests: every subcommand name is recognized by clap
// ---------------------------------------------------------------------------

#[test]
fn test_patch_malformed_file_fails() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "content\n").unwrap();

    let patch_file = dir.path().join("bad.patch");
    fs::write(&patch_file, "this is not a valid unified diff\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .assert()
        .code(4)
        .stderr(predicate::str::contains("parse error"));
}

// Regression: merge-check exit code was SUCCESS when both a conflict (resolved
// via --allow-conflicts) and a hard error (hunk not locatable) occurred in a
// multi-file patch.  The error was masked because `has_conflicts && allow_conflicts`
// short-circuited before checking for errors.
#[test]
fn test_patch_merge_check_allow_conflicts_still_exits_5_on_error() {
    let dir = TempDir::new().unwrap();
    // File A: will produce a conflict (content diverged from patch context).
    let file_a = dir.path().join("a.txt");
    fs::write(&file_a, "line1\ncompletely different\nline3\n").unwrap();
    // File B: does not exist, so merge will fail entirely (error status).

    let patch_file = dir.path().join("multi.patch");
    fs::write(
        &patch_file,
        "\
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,3 @@
 line1
-old line
+new line
 line3
--- a/b.txt
+++ b/b.txt
@@ -1,3 +1,3 @@
 alpha
-beta
+gamma
 delta
",
    )
    .unwrap();
    // With --allow-conflicts, the conflict on a.txt is acceptable, but the
    // error on b.txt should still produce a non-zero exit code (AMBIGUOUS=5).
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("merge")
        .arg(&patch_file)
        .arg("--check")
        .arg("--allow-conflicts")
        .assert()
        .code(5);
}

// ---------------------------------------------------------------------------
// --jsonl + count/files-with-matches (bug fix: count_only + jsonl)
// ---------------------------------------------------------------------------
