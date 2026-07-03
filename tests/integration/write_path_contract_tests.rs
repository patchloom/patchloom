//! Write-path contract matrix for Phase 1 of #1373 / epic #1372.
//!
//! Locks exit codes and mutation semantics for each of the five production
//! write entry paths when changes exist:
//!
//! | Mode | Exit | Mutates |
//! |------|-----:|---------|
//! | preview (default) | 2 | no |
//! | --check | 2 | no |
//! | --apply | 0 | yes |
//! | --confirm --json (non-TTY decline) | 2 | no |
//!
//! Inventory: `docs/plans/write-pipeline.md`.
//!
//! These tests intentionally use one *representative* CLI command per path.
//! Broader coverage lives in per-command integration modules; this file is
//! the regression lock for the multi-path exit-code class (#1345–#1348).

use super::*;

/// Exit code constants (must match `src/exit.rs`).
const SUCCESS: i32 = 0;
const CHANGES_DETECTED: i32 = 2;

/// How write flags are applied for a contract scenario.
#[derive(Clone, Copy, Debug)]
enum Mode {
    Preview,
    Check,
    Apply,
    /// `--confirm` with `--json`; non-TTY decline (`should_apply` == false).
    ConfirmJsonDecline,
}

impl Mode {
    fn write_flags(self) -> &'static [&'static str] {
        match self {
            Mode::Preview => &[],
            Mode::Check => &["--check"],
            Mode::Apply => &["--apply"],
            Mode::ConfirmJsonDecline => &["--confirm"],
        }
    }

    fn wants_json(self) -> bool {
        matches!(self, Mode::ConfirmJsonDecline)
    }

    fn expected_exit(self) -> i32 {
        match self {
            Mode::Apply => SUCCESS,
            Mode::Preview | Mode::Check | Mode::ConfirmJsonDecline => CHANGES_DETECTED,
        }
    }

    fn expects_mutation(self) -> bool {
        matches!(self, Mode::Apply)
    }
}

/// Shared assertion helper for a write command with pending changes.
///
/// `configure` adds the subcommand and its positional/option args (not write
/// flags, not `--cwd` / `--json`). Write and global flags are applied here so
/// clap global-order rules stay correct.
fn assert_write_path_contract(
    path_label: &str,
    dir: &TempDir,
    configure: impl Fn(&mut assert_cmd::Command),
    mutated: impl Fn() -> bool,
    reset: impl Fn(),
) {
    for mode in [
        Mode::Preview,
        Mode::Check,
        Mode::ConfirmJsonDecline,
        Mode::Apply, // last: mutates the fixture
    ] {
        reset();
        assert!(
            !mutated(),
            "{path_label}/{mode:?}: fixture must start unmutated"
        );

        let mut cmd = Command::cargo_bin("patchloom").unwrap();
        cmd.arg("--cwd").arg(dir.path());
        if mode.wants_json() {
            cmd.arg("--json");
        }
        configure(&mut cmd);
        for f in mode.write_flags() {
            cmd.arg(f);
        }

        cmd.assert().code(mode.expected_exit());

        if mode.expects_mutation() {
            assert!(
                mutated(),
                "{path_label}/{mode:?}: expected mutation after exit {}",
                mode.expected_exit()
            );
        } else {
            assert!(
                !mutated(),
                "{path_label}/{mode:?}: must not mutate (exit {} contract)",
                mode.expected_exit()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Path 1: execute_via_engine  (representative: create)
// ---------------------------------------------------------------------------

#[test]
fn contract_execute_via_engine_create() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("via_engine.txt");

    assert_write_path_contract(
        "execute_via_engine/create",
        &dir,
        |cmd| {
            cmd.arg("create")
                .arg("via_engine.txt")
                .arg("--content")
                .arg("hello\n");
        },
        || file.exists() && fs::read_to_string(&file).unwrap() == "hello\n",
        || {
            let _ = fs::remove_file(&file);
        },
    );
}

// ---------------------------------------------------------------------------
// Path 1 variant: execute_via_engine_no_preview_diffs  (representative: delete)
// ---------------------------------------------------------------------------

#[test]
fn contract_execute_via_engine_no_preview_diffs_delete() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("via_engine_delete.txt");

    assert_write_path_contract(
        "execute_via_engine_no_preview_diffs/delete",
        &dir,
        |cmd| {
            cmd.arg("delete").arg("via_engine_delete.txt");
        },
        || !file.exists(),
        || {
            fs::write(&file, "remove me\n").unwrap();
        },
    );
}

// ---------------------------------------------------------------------------
// Path 2: execute_operations  (representative: tidy multi-file fix)
// ---------------------------------------------------------------------------

#[test]
fn contract_execute_operations_tidy() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("ops_tidy.txt");
    // Missing final newline → tidy --ensure-final-newline has work to do.
    let dirty = "line without final newline";
    let clean = "line without final newline\n";

    assert_write_path_contract(
        "execute_operations/tidy",
        &dir,
        |cmd| {
            cmd.arg("tidy")
                .arg("fix")
                .arg("ops_tidy.txt")
                .arg("--ensure-final-newline");
        },
        || {
            fs::read_to_string(&file)
                .map(|s| s == clean)
                .unwrap_or(false)
        },
        || {
            fs::write(&file, dirty).unwrap();
        },
    );
}

// ---------------------------------------------------------------------------
// Path 3: execute_precomputed  (representative: replace scan)
// ---------------------------------------------------------------------------

#[test]
fn contract_execute_precomputed_replace() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("precomputed_replace.txt");
    let before = "hello world\n";
    let after = "hello patchloom\n";

    assert_write_path_contract(
        "execute_precomputed/replace",
        &dir,
        |cmd| {
            cmd.arg("replace")
                .arg("world")
                .arg("--new")
                .arg("patchloom")
                .arg("precomputed_replace.txt");
        },
        || {
            fs::read_to_string(&file)
                .map(|s| s == after)
                .unwrap_or(false)
        },
        || {
            fs::write(&file, before).unwrap();
        },
    );
}

// ---------------------------------------------------------------------------
// Path 4: execute_write (write_dispatch)  (representative: binary rename)
// ---------------------------------------------------------------------------

#[test]
fn contract_execute_write_binary_rename() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("binary_src.bin");
    let dst = dir.path().join("binary_dst.bin");
    // NUL byte forces the binary rename path (execute_write), not the tx engine.
    let payload = b"bin\0ary";

    assert_write_path_contract(
        "execute_write/binary_rename",
        &dir,
        |cmd| {
            cmd.arg("rename")
                .arg("binary_src.bin")
                .arg("binary_dst.bin");
        },
        || dst.exists() && !src.exists() && fs::read(&dst).unwrap() == payload,
        || {
            let _ = fs::remove_file(&dst);
            fs::write(&src, payload).unwrap();
        },
    );
}

// ---------------------------------------------------------------------------
// Path 5: execute_single + custom CLI mode branch  (representative: patch apply)
// ---------------------------------------------------------------------------

#[test]
fn contract_execute_single_custom_mode_patch() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("patched.txt");
    let patch = dir.path().join("change.patch");
    let before = "line1\nold line\nline3\n";
    let after = "line1\nnew line\nline3\n";
    let patch_body = "--- a/patched.txt\n+++ b/patched.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n";

    // Use absolute patch path: some patch loaders resolve the patch file
    // against process CWD rather than --cwd (lock current behavior for the
    // exit/mutation contract, not path resolution).
    let patch_path = patch.clone();
    assert_write_path_contract(
        "execute_single_custom/patch",
        &dir,
        |cmd| {
            cmd.arg("patch").arg("apply").arg(patch_path.as_os_str());
        },
        || {
            fs::read_to_string(&file)
                .map(|s| s == after)
                .unwrap_or(false)
        },
        || {
            fs::write(&file, before).unwrap();
            fs::write(&patch, patch_body).unwrap();
        },
    );
}
