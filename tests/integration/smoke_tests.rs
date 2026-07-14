use super::*;

#[test]
fn test_smoke_example_01_basic_replace_plan() {
    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("01-basic-replace.json"))
        .arg("--apply")
        .assert()
        .code(0);

    let readme = fs::read_to_string(dir.path().join("README.md")).unwrap();
    assert!(readme.contains("v2.0.0"));
    assert!(!readme.contains("v1.0.0"));
}

#[test]
fn test_smoke_example_02_multi_file_batch_plan() {
    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("02-multi-file-batch.json"))
        .arg("--apply")
        .assert()
        .code(0);

    let cargo_toml = fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("version = \"0.2.0\""));
    assert!(!cargo_toml.contains("version = \"0.1.0\""));

    let package: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package["version"], "0.2.0");

    let readme = fs::read_to_string(dir.path().join("README.md")).unwrap();
    assert!(readme.contains("v0.2.0"));
    assert!(!readme.contains("v0.1.0"));
}

#[test]
fn test_smoke_example_03_markdown_editing_plan() {
    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("03-markdown-editing.json"))
        .arg("--apply")
        .assert()
        .code(0);

    let changelog = fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
    assert!(changelog.contains("- Added new feature"));
    assert!(changelog.contains("- Fixed bug in parser"));
    assert!(!changelog.contains("- Existing entry"));

    let agents = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert_eq!(agents.matches("## Safety rules").count(), 1);
    assert!(agents.contains("Always run `make check` before committing"));
    // md.move_section moved FAQ before License.
    let faq_pos = agents
        .find("## FAQ")
        .expect("FAQ section should exist after move");
    let license_pos = agents
        .find("## License")
        .expect("License section should exist");
    assert!(
        faq_pos < license_pos,
        "FAQ should appear before License after move"
    );

    let readme = fs::read_to_string(dir.path().join("README.md")).unwrap();
    assert!(readme.contains("| `new-cmd` | Description of the new command |"));
}

#[test]
fn test_smoke_example_04_doc_mutations_plan() {
    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("04-doc-mutations.json"))
        .arg("--apply")
        .assert()
        .code(0);

    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("config.json")).unwrap()).unwrap();
    assert_eq!(config["database"]["port"], 5432);
    assert_eq!(config["database"]["pool_size"], 10);
    assert_eq!(config["logging"]["level"], "info");
    assert_eq!(config["logging"]["format"], "json");
    assert!(
        config["allowed_origins"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str() == Some("https://example.com"))
    );
    assert!(config.get("deprecated_field").is_none());

    let yaml = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
    assert!(yaml.contains("active: true"));
    assert!(!yaml.contains("active: false"));
    assert!(!yaml.contains("bob"));
}

#[test]
fn test_smoke_example_05_strict_mode_plan() {
    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("05-strict-mode.json"))
        .arg("--apply")
        .assert()
        .code(0);

    let new_module = fs::read_to_string(dir.path().join("src/new_module.rs")).unwrap();
    assert!(new_module.contains("pub fn hello() -> &'static str"));

    let lib_rs = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
    assert!(lib_rs.contains("pub mod new_module;"));

    let changelog = fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
    assert!(changelog.contains("- Added new_module"));
}

#[test]
fn test_smoke_example_06_batch_version_bump() {
    let dir = TempDir::new().unwrap();

    // Create fixture files matching the batch example operations.
    fs::write(
        dir.path().join("package.json"),
        "{\n  \"version\": \"1.9.0\"\n}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nversion = \"1.9.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nversion = \"1.9.0\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("config")).unwrap();
    fs::write(
        dir.path().join("config/settings.yaml"),
        "app:\n  version: \"1.9.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("README.md"),
        "# Project\n\n![version](https://img.shields.io/badge/version-1.9.0-blue)\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("CHANGELOG.md"),
        "# Changelog\n\n## Unreleased\n\n## v1.9.0\n- Initial\n",
    )
    .unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(example_plan_path("06-batch-version-bump.txt"))
        .arg("--apply")
        .assert()
        .code(0);

    let pkg = fs::read_to_string(dir.path().join("package.json")).unwrap();
    assert!(pkg.contains("2.0.0"), "package.json not updated: {pkg}");

    let cargo = fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
    assert!(cargo.contains("2.0.0"), "Cargo.toml not updated: {cargo}");

    let readme = fs::read_to_string(dir.path().join("README.md")).unwrap();
    assert!(
        readme.contains("version-2.0.0-blue"),
        "README badge not updated: {readme}"
    );

    let pyproject = fs::read_to_string(dir.path().join("pyproject.toml")).unwrap();
    assert!(
        pyproject.contains("2.0.0"),
        "pyproject.toml not updated: {pyproject}"
    );

    let settings = fs::read_to_string(dir.path().join("config/settings.yaml")).unwrap();
    assert!(
        settings.contains("2.0.0"),
        "config/settings.yaml not updated: {settings}"
    );

    let changelog = fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
    assert!(
        changelog.contains("Bump version to 2.0.0"),
        "CHANGELOG bullet not added: {changelog}"
    );
}

#[test]
fn test_smoke_example_07_yaml_plan() {
    let dir = TempDir::new().unwrap();

    // Create fixture files matching the YAML plan operations.
    fs::write(
        dir.path().join("config.yaml"),
        "app:\n  version: \"1.0.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("CHANGELOG.md"),
        "# Changelog\n\n## Unreleased\n\n- Initial\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("README.md"),
        "# Project v1.0.0\n\nUsing v1.0.0 everywhere.\n",
    )
    .unwrap();

    // --diff mode: plan should parse successfully and exit 2 (changes detected).
    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("07-yaml-plan.yaml"))
        .arg("--diff")
        .assert()
        .code(2);

    // --apply: plan sets strict: false so ops land even if format/validate tools
    // (prettier/yamllint) are missing. Exit 0 when tools exist; exit 6
    // (VALIDATION_FAILED) when they do not. Never discard the exit code.
    let apply = patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("07-yaml-plan.yaml"))
        .arg("--apply")
        .output()
        .expect("tx --apply should run");
    let code = apply.status.code().unwrap_or(1);
    assert!(
        code == 0 || code == 6,
        "expected exit 0 (format tools present) or 6 (missing prettier/yamllint), got {code}: {}",
        String::from_utf8_lossy(&apply.stderr)
    );

    let config = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
    assert!(
        config.contains("2.0.0"),
        "config.yaml version not updated: {config}"
    );
}

#[test]
fn test_smoke_example_09_patch_apply() {
    let dir = TempDir::new().unwrap();

    // Create the fixture file that the patch targets.
    fs::create_dir_all(dir.path().join("src")).unwrap();
    // Fixture must match the line numbers in the patch hunks:
    // Hunk 1: @@ -10,7  =>  pub struct Config { at line 10
    // Hunk 2: @@ -20,6  =>  Config { at line 20
    fs::write(
        dir.path().join("src/config.rs"),
        "// Config module\n\n\
         use std::time::Duration;\n\n\
         const MAX_RETRIES: u32 = 5;\n\
         const DEFAULT_PORT: u16 = 8080;\n\n\
         /// Application configuration.\n\
         #[derive(Debug, Clone)]\n\
         pub struct Config {\n\
         \x20\x20\x20\x20pub host: String,\n\
         \x20\x20\x20\x20pub port: u16,\n\
         \x20\x20\x20\x20pub timeout: u64,\n\
         \x20\x20\x20\x20pub retries: u32,\n\
         }\n\n\
         impl Default for Config {\n\
         \x20\x20\x20\x20fn default() -> Self {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// Create with sensible defaults\n\
         \x20\x20\x20\x20\x20\x20\x20\x20Config {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20host: \"localhost\".to_string(),\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20port: 8080,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20timeout: 30,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20retries: 3,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}\n\
         \x20\x20\x20\x20}\n\
         }\n",
    )
    .unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("09-patch-apply.json"))
        .arg("--diff")
        .assert()
        .code(2);
}

#[test]
fn test_smoke_example_10_inspect_and_edit() {
    let dir = TempDir::new().unwrap();

    // Create fixture files matching the plan's operations.
    fs::write(
        dir.path().join("config.json"),
        "{\n  \"database\": {\n    \"port\": 5432\n  }\n}\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/db.rs"),
        "const DB_PORT: u16 = 5432;\n\nfn connect() {\n    // ...\n}\n",
    )
    .unwrap();

    // --diff mode: should parse and preview changes.
    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("10-inspect-and-edit.json"))
        .arg("--diff")
        .assert()
        .code(2);

    // --apply: verify writes landed.
    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("10-inspect-and-edit.json"))
        .arg("--apply")
        .assert()
        .code(0);

    let config = fs::read_to_string(dir.path().join("config.json")).unwrap();
    assert!(
        config.contains("5433"),
        "config.json port not updated: {config}"
    );

    let db = fs::read_to_string(dir.path().join("src/db.rs")).unwrap();
    assert!(
        db.contains("DB_PORT: u16 = 5433"),
        "db.rs not updated: {db}"
    );
}

#[test]
fn test_smoke_example_08_mcp_tool_names_valid() {
    // Parse example 08 and verify every tool name exists in the MCP tool list
    // produced by `patchloom agent-rules --mode mcp`.
    let example = fs::read_to_string(example_plan_path("08-mcp-tool-call.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&example).unwrap();
    let tool_names: Vec<&str> = doc["examples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["tool"].as_str().unwrap())
        .collect();
    assert!(!tool_names.is_empty(), "example 08 has no tool examples");

    // Get the authoritative tool list from agent-rules --mode mcp.
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["agent-rules", "--mode", "mcp"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let rules = String::from_utf8(output.stdout).unwrap();

    for name in &tool_names {
        assert!(
            rules.contains(name),
            "example 08 references tool '{name}' which is not in agent-rules --mode mcp output"
        );
    }
}

// ── Batch integration tests ───────────────────────────────────────────

#[test]
fn test_smoke_quickstart_command_flow() {
    let quickstart = fs::read_to_string(quickstart_path()).unwrap();
    assert!(quickstart.contains("patchloom search 'TODO' src/"));
    assert!(quickstart.contains("patchloom search 'TODO' --count src/"));
    assert!(quickstart.contains("patchloom replace 'old_function' --new 'new_function' src/"));
    assert!(
        quickstart.contains("patchloom replace 'old_function' --new 'new_function' src/ --apply")
    );
    assert!(quickstart.contains("patchloom init"));
    assert!(quickstart.contains("appends the rules to an existing agent instructions file"));
    assert!(quickstart.contains(".vscode/mcp.json"));
    assert!(quickstart.contains(".cursor/mcp.json"));
    assert!(quickstart.contains("patchloom doc get package.json version"));
    assert!(quickstart.contains("patchloom doc set package.json version \"2.0.0\" --apply"));
    assert!(quickstart.contains("patchloom batch <<'EOF'"));
    assert!(quickstart.contains("patchloom batch --apply <<'EOF'"));
    assert!(quickstart.contains("patchloom status"));
    assert!(quickstart.contains("`patchloom status` is git-backed."));
    assert!(quickstart.contains("patchloom undo"));
    assert!(quickstart.contains("patchloom undo --apply"));

    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());
    git_ok(dir.path(), &["init"]);
    git_ok(dir.path(), &["config", "user.email", "test@test.com"]);
    git_ok(dir.path(), &["config", "user.name", "Test"]);
    git_ok(
        dir.path(),
        &[
            "add",
            "Cargo.toml",
            "src",
            "README.md",
            "CHANGELOG.md",
            "AGENTS.md",
            "package.json",
            "config.json",
            "config.yaml",
        ],
    );
    git_ok(dir.path(), &["commit", "-m", "init"]);
    let lib_path = dir.path().join("src/lib.rs");

    patchloom_in(dir.path())
        .arg("search")
        .arg("TODO")
        .arg("src/")
        .assert()
        .success()
        .stdout(predicate::str::contains("TODO"));

    patchloom_in(dir.path())
        .arg("search")
        .arg("TODO")
        .arg("--count")
        .arg("src/")
        .assert()
        .success()
        .stdout(predicate::str::contains("lib.rs:2"));

    patchloom_in(dir.path())
        .arg("replace")
        .arg("old_function")
        .arg("--new")
        .arg("new_function")
        .arg("src/")
        .assert()
        .code(2) // CHANGES_DETECTED: preview mode, not applied
        .stdout(predicate::str::contains("new_function"));
    assert!(
        fs::read_to_string(&lib_path)
            .unwrap()
            .contains("old_function")
    );

    patchloom_in(dir.path())
        .arg("replace")
        .arg("old_function")
        .arg("--new")
        .arg("new_function")
        .arg("src/")
        .arg("--apply")
        .assert()
        .code(0);
    assert!(
        fs::read_to_string(&lib_path)
            .unwrap()
            .contains("new_function")
    );
    assert!(
        !fs::read_to_string(&lib_path)
            .unwrap()
            .contains("old_function")
    );

    patchloom_in(dir.path())
        .arg("doc")
        .arg("get")
        .arg("package.json")
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("1.0.0"));

    patchloom_in(dir.path())
        .arg("doc")
        .arg("set")
        .arg("package.json")
        .arg("version")
        .arg("\"2.0.0\"")
        .arg("--apply")
        .assert()
        .code(0);

    let package: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package["version"], "2.0.0");

    let batch_preview = patchloom_in(dir.path())
        .arg("batch")
        .write_stdin(
            "doc.set package.json version \"3.0.0\"\nreplace README.md \"v1.0.0\" \"v3.0.0\"\nmd.insert_after_heading CHANGELOG.md \"## Unreleased\" \"- Bumped to v3.0.0\"\n",
        )
        .output()
        .unwrap();
    assert_eq!(
        batch_preview.status.code(),
        Some(2),
        "batch preview should exit 2"
    );
    let batch_preview_stdout = String::from_utf8_lossy(&batch_preview.stdout);
    assert!(batch_preview_stdout.contains("package.json"));
    assert!(batch_preview_stdout.contains("README.md"));
    assert!(batch_preview_stdout.contains("CHANGELOG.md"));

    let batch_apply = patchloom_in(dir.path())
        .arg("batch")
        .arg("--apply")
        .write_stdin(
            "doc.set package.json version \"3.0.0\"\nreplace README.md \"v1.0.0\" \"v3.0.0\"\nmd.insert_after_heading CHANGELOG.md \"## Unreleased\" \"- Bumped to v3.0.0\"\n",
        )
        .output()
        .unwrap();
    assert!(batch_apply.status.success());

    let package_after_batch: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package_after_batch["version"], "3.0.0");
    assert!(
        fs::read_to_string(dir.path().join("README.md"))
            .unwrap()
            .contains("v3.0.0")
    );
    assert!(
        fs::read_to_string(dir.path().join("CHANGELOG.md"))
            .unwrap()
            .contains("- Bumped to v3.0.0")
    );

    let status_output = patchloom_in(dir.path()).arg("status").output().unwrap();
    assert_eq!(status_output.status.code(), Some(2));
    let status_stdout = String::from_utf8_lossy(&status_output.stdout);
    assert!(status_stdout.contains("README.md"));
    assert!(status_stdout.contains("package.json"));

    patchloom_in(dir.path())
        .arg("undo")
        .assert()
        .code(2)
        .stdout(predicate::str::contains("Would restore session"));

    patchloom_in(dir.path())
        .arg("undo")
        .arg("--apply")
        .assert()
        .code(0);

    let package_after_undo: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package_after_undo["version"], "2.0.0");
    assert!(
        fs::read_to_string(dir.path().join("README.md"))
            .unwrap()
            .contains("v1.0.0")
    );
    assert!(
        !fs::read_to_string(dir.path().join("CHANGELOG.md"))
            .unwrap()
            .contains("- Bumped to v3.0.0")
    );
}

#[test]
fn test_smoke_quickstart_transaction_snippet() {
    let quickstart = fs::read_to_string(quickstart_path()).unwrap();
    let plan_text = extract_markdown_code_block_after(
        &quickstart,
        "Create a plan file called `bump.json`:",
        "json",
    );
    let expected_json: serde_json::Value = serde_json::from_str(
        &extract_markdown_code_block_after(&quickstart, "Returns:", "json"),
    )
    .unwrap();

    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());
    let plan_file = dir.path().join("bump.json");
    fs::write(&plan_file, plan_text).unwrap();

    let diff_output = patchloom_in(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .output()
        .unwrap();
    assert_eq!(diff_output.status.code(), Some(2), "tx diff should exit 2");
    let diff_stdout = String::from_utf8_lossy(&diff_output.stdout);
    assert!(diff_stdout.contains("package.json"));
    assert!(diff_stdout.contains("README.md"));
    assert!(diff_stdout.contains("CHANGELOG.md"));

    let package_before: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package_before["version"], "1.0.0");
    assert!(
        fs::read_to_string(dir.path().join("README.md"))
            .unwrap()
            .contains("v1.0.0")
    );
    assert!(
        !fs::read_to_string(dir.path().join("CHANGELOG.md"))
            .unwrap()
            .contains("- Bumped to v2.0.0")
    );

    let check_output = patchloom_in(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--check")
        .output()
        .unwrap();
    assert_eq!(check_output.status.code(), Some(2));

    let apply_output = patchloom_in(dir.path())
        .arg("--json")
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .output()
        .unwrap();
    assert!(apply_output.status.success());
    let actual_json: serde_json::Value = serde_json::from_slice(&apply_output.stdout).unwrap();
    assert_eq!(actual_json, expected_json);

    let package_after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package_after["version"], "2.0.0");
    assert!(
        fs::read_to_string(dir.path().join("README.md"))
            .unwrap()
            .contains("v2.0.0")
    );
    assert!(
        fs::read_to_string(dir.path().join("CHANGELOG.md"))
            .unwrap()
            .contains("- Bumped to v2.0.0")
    );
}

#[test]
fn test_smoke_shell_completion_docs_include_elvish() {
    // Shell completions docs live in the installation guide (not README).
    let content = fs::read_to_string(installation_path()).unwrap();
    assert!(
        content.contains("patchloom completions elvish"),
        "installation guide should document elvish completions"
    );
}

#[test]
fn test_smoke_source_install_docs_use_cargo_install_path() {
    let source_install_flow = "git clone https://github.com/patchloom/patchloom.git\ncd patchloom\ncargo install --path .";

    // Source install flow lives in the installation guide (not README,
    // which leads with Homebrew/crates.io and links to the guide).
    let content = fs::read_to_string(installation_path()).unwrap();
    assert!(
        content.contains(source_install_flow),
        "installation guide should document the source install flow"
    );

    // README should reference the installation guide for details.
    let readme = fs::read_to_string(readme_path()).unwrap();
    assert!(
        readme.contains("[Installation](./docs/getting-started/installation.md)"),
        "README should link to the full installation guide"
    );
}

#[test]
fn test_smoke_installation_docs_confirm_mcp_included_by_default() {
    let content = fs::read_to_string(installation_path()).unwrap();
    assert!(
        content.contains("all features by default"),
        "installation guide should confirm all features are included by default"
    );
    assert!(
        content.contains("--no-default-features"),
        "installation guide should document opting out of MCP"
    );
}

#[test]
fn test_smoke_installation_docs_cover_contributor_verification_loop() {
    let content = fs::read_to_string(installation_path()).unwrap();
    assert!(
        content.contains("make check-fast"),
        "installation guide should mention the fast contributor iteration loop"
    );
    assert!(
        content.contains("make check"),
        "installation guide should mention the full contributor verification gate"
    );
}

#[test]
fn test_smoke_rust_version_docs_and_ci_match_cargo_metadata() {
    let cargo = fs::read_to_string(repo_root().join("Cargo.toml")).unwrap();
    let rust_version_line = cargo
        .lines()
        .find(|line| line.starts_with("rust-version = "))
        .expect("Cargo.toml should declare rust-version");
    let rust_version = rust_version_line
        .split('"')
        .nth(1)
        .expect("rust-version should be quoted");

    let readme = fs::read_to_string(readme_path()).unwrap();
    assert!(
        readme.contains(&format!("Rust {rust_version}+")),
        "README should advertise the same Rust minimum version as Cargo.toml"
    );

    let contributing = fs::read_to_string(repo_root().join("CONTRIBUTING.md")).unwrap();
    assert!(
        contributing.contains(&format!("Rust {rust_version}+")),
        "CONTRIBUTING.md should advertise the same Rust minimum version as Cargo.toml"
    );

    let changelog = fs::read_to_string(repo_root().join("CHANGELOG.md")).unwrap();
    assert!(
        changelog.contains(&format!("MSRV: Rust {rust_version}+")),
        "CHANGELOG.md should advertise the same Rust minimum version as Cargo.toml"
    );

    let installation = fs::read_to_string(installation_path()).unwrap();
    assert!(
        installation.contains(&format!("requires Rust {rust_version}+")),
        "installation guide should advertise the same Rust minimum version as Cargo.toml"
    );

    let ci = fs::read_to_string(ci_workflow_path()).unwrap();
    assert!(
        ci.contains(&format!("toolchain: \"{rust_version}\"")),
        "ci.yml should pin the MSRV job to the same Rust version as Cargo.toml"
    );
}

#[test]
fn test_smoke_readme_command_examples() {
    // README links to the reference doc; detailed examples live there.
    let readme = fs::read_to_string(readme_path()).unwrap();
    assert!(
        readme.contains("patchloom init"),
        "README quick start should lead with patchloom init"
    );
    assert!(
        readme.contains("appends the rules to an existing agent instructions file"),
        "README should describe init append behavior for existing agent instruction files"
    );
    assert!(
        readme.contains(".vscode/mcp.json"),
        "README should mention the VS Code MCP config path"
    );
    assert!(
        readme.contains(".cursor/mcp.json"),
        "README should mention the Cursor MCP config path"
    );
    assert!(
        readme.contains("docs/reference/README.md"),
        "README should link to the command reference"
    );
    // Verify the reference doc contains the detailed examples.
    let reference = fs::read_to_string(repo_root().join("docs/reference/README.md")).unwrap();
    assert!(reference.contains(".vscode/mcp.json"));
    assert!(reference.contains(".cursor/mcp.json"));
    assert!(reference.contains("`search`"));
    assert!(reference.contains("`replace`"));
    assert!(reference.contains("`doc`"));
    assert!(reference.contains("`tidy`"));
    let launch = fs::read_to_string(launch_announcement_path()).unwrap();
    assert!(launch.contains("appends the rules to an existing agent instructions file"));
    assert!(launch.contains(".vscode/mcp.json"));
    assert!(launch.contains(".cursor/mcp.json"));
    assert!(launch.contains("2,800+ tests"));
    assert!(
        launch.contains("23 commands"),
        "launch announcement CLI command count drifted"
    );
    assert!(
        launch.contains("56 structured tool calls"),
        "launch announcement MCP tool count drifted"
    );
    let merge_value = r#"{"settings": {"debug": true}}"#;

    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("search")
        .arg("TODO")
        .arg("src/")
        .assert()
        .success()
        .stdout(predicate::str::contains("TODO"));

    patchloom_in(dir.path())
        .arg("replace")
        .arg("old_name")
        .arg("--new")
        .arg("new_name")
        .arg("src/")
        .arg("--apply")
        .assert()
        .code(0);
    let rename_module = fs::read_to_string(dir.path().join("src/rename.rs")).unwrap();
    assert!(rename_module.contains("new_name"));
    assert!(!rename_module.contains("old_name"));

    patchloom_in(dir.path())
        .arg("doc")
        .arg("keys")
        .arg("package.json")
        .arg(".")
        .assert()
        .success()
        .stdout(predicate::str::contains("name"))
        .stdout(predicate::str::contains("version"));

    patchloom_in(dir.path())
        .arg("doc")
        .arg("merge")
        .arg("config.json")
        .arg("--value")
        .arg(merge_value)
        .arg("--apply")
        .assert()
        .code(0);
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("config.json")).unwrap()).unwrap();
    assert_eq!(config["settings"]["debug"], true);

    let tidy_target = dir.path().join("notes.txt");
    fs::write(&tidy_target, "line with space \nsecond line").unwrap();

    patchloom_in(dir.path())
        .arg("tidy")
        .arg("check")
        .arg("notes.txt")
        .assert()
        .code(2);

    patchloom_in(dir.path())
        .arg("tidy")
        .arg("fix")
        .arg(".")
        .arg("--ensure-final-newline")
        .arg("--apply")
        .assert()
        .code(0);
    assert!(fs::read(&tidy_target).unwrap().ends_with(b"\n"));
}
