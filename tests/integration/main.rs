use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Convert a path to a string safe for embedding in YAML/TOML values.
/// On Windows, backslashes in paths like `C:\Users\...` are interpreted
/// as escape sequences (`\U` = unicode escape). Forward slashes work
/// fine on Windows and avoid the problem.
fn portable_path_str(p: &Path) -> String {
    p.to_str().unwrap().replace('\\', "/")
}

fn nonexistent_path(name: &str) -> String {
    #[cfg(windows)]
    {
        format!("C:\\patchloom-nonexistent-{name}")
    }
    #[cfg(not(windows))]
    {
        format!("/tmp/patchloom-nonexistent-{name}")
    }
}

fn shell_false() -> &'static str {
    #[cfg(windows)]
    {
        "exit /b 1"
    }
    #[cfg(not(windows))]
    {
        "false"
    }
}

fn shell_exit_1() -> &'static str {
    #[cfg(windows)]
    {
        "exit /b 1"
    }
    #[cfg(not(windows))]
    {
        "exit 1"
    }
}

fn shell_sleep_300() -> &'static str {
    #[cfg(windows)]
    {
        // ping localhost 301 times (1 second apart) to sleep ~300 seconds.
        // Redirect to nul to suppress output.
        "ping -n 301 127.0.0.1 > nul"
    }
    #[cfg(not(windows))]
    {
        "sleep 300"
    }
}

fn shell_touch(path: &Path) -> String {
    #[cfg(windows)]
    {
        let path = path.to_str().unwrap();
        // type nul produces empty output; > redirects it to create the file.
        format!("type nul > \"{path}\"")
    }
    #[cfg(not(windows))]
    {
        let path = path.display();
        format!("touch '{path}'")
    }
}

fn shell_fail_with_secret(secret: &str) -> String {
    #[cfg(windows)]
    {
        format!("cmd /C \"set PATCHLOOM_SECRET={secret}&& exit /b 1\"")
    }
    #[cfg(not(windows))]
    {
        format!("PATCHLOOM_SECRET='{secret}' false")
    }
}

fn shell_test_exists(path: &Path) -> String {
    #[cfg(windows)]
    {
        let path = path.to_str().unwrap();
        format!("if exist \"{path}\" (exit /b 0) else (exit /b 1)")
    }
    #[cfg(not(windows))]
    {
        let path = path.display();
        format!("test -f '{path}'")
    }
}

#[cfg(unix)]
fn run_patchloom_confirm_in_pty_with_env(
    args: &[&str],
    input: &str,
    env: &[(&str, &str)],
) -> std::process::Output {
    let python = r#"
import json, os, pty, subprocess, sys

args = json.loads(os.environ["PATCHLOOM_PTY_ARGS"])
cwd = os.environ["PATCHLOOM_PTY_CWD"]
input_data = os.environ["PATCHLOOM_PTY_INPUT"].encode()
child_env = os.environ.copy()
child_env.update(json.loads(os.environ["PATCHLOOM_PTY_ENV"]))

master_fd, slave_fd = pty.openpty()
proc = subprocess.Popen(
    args,
    cwd=cwd,
    stdin=slave_fd,
    stdout=slave_fd,
    stderr=slave_fd,
    env=child_env,
)
os.close(slave_fd)
if input_data:
    os.write(master_fd, input_data)

output = bytearray()
while True:
    try:
        chunk = os.read(master_fd, 4096)
        if not chunk:
            break
        output.extend(chunk)
    except OSError:
        break

status = proc.wait()
os.close(master_fd)
sys.stdout.buffer.write(output)
sys.exit(status)
"#;
    let full_args = std::iter::once(env!("CARGO_BIN_EXE_patchloom").to_string())
        .chain(args.iter().map(|arg| (*arg).to_string()))
        .collect::<Vec<_>>();
    let env_json = env
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect::<std::collections::BTreeMap<_, _>>();

    std::process::Command::new("python3")
        .arg("-c")
        .arg(python)
        .env(
            "PATCHLOOM_PTY_ARGS",
            serde_json::to_string(&full_args).unwrap(),
        )
        .env("PATCHLOOM_PTY_CWD", repo_root())
        .env("PATCHLOOM_PTY_INPUT", input)
        .env(
            "PATCHLOOM_PTY_ENV",
            serde_json::to_string(&env_json).unwrap(),
        )
        .output()
        .unwrap()
}

#[cfg(unix)]
fn run_patchloom_confirm_in_pty(args: &[&str], input: &str) -> std::process::Output {
    run_patchloom_confirm_in_pty_with_env(args, input, &[])
}

fn assert_patch_apply_error_object(
    output: &std::process::Output,
    expected_exit_code: i32,
    expected_error_substring: &str,
) {
    assert_eq!(output.status.code(), Some(expected_exit_code));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains(expected_error_substring)
    );
}

fn git_ok(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:{}\nstderr:{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_git_repo_with_committed_file(dir: &Path, file: &str, content: &str) {
    git_ok(dir, &["init"]);
    git_ok(dir, &["config", "user.email", "test@test.com"]);
    git_ok(dir, &["config", "user.name", "Test"]);
    fs::write(dir.join(file), content).unwrap();
    git_ok(dir, &["add", file]);
    git_ok(dir, &["commit", "-m", "init"]);
}

// ---------------------------------------------------------------------------
// search
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Shared test helpers (used across multiple test modules)
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn example_plan_path(name: &str) -> PathBuf {
    repo_root().join("examples").join(name)
}

fn quickstart_path() -> PathBuf {
    repo_root()
        .join("docs")
        .join("getting-started")
        .join("quickstart.md")
}

fn installation_path() -> PathBuf {
    repo_root()
        .join("docs")
        .join("getting-started")
        .join("installation.md")
}

fn agent_test_readme_path() -> PathBuf {
    repo_root().join("tests").join("agent").join("README.md")
}

fn concepts_path() -> PathBuf {
    repo_root()
        .join("docs")
        .join("getting-started")
        .join("concepts.md")
}

fn ci_workflow_path() -> PathBuf {
    repo_root().join(".github").join("workflows").join("ci.yml")
}

fn readme_path() -> PathBuf {
    repo_root().join("README.md")
}

fn launch_announcement_path() -> PathBuf {
    repo_root()
        .join("docs")
        .join("blog")
        .join("launch-announcement.md")
}

fn patchloom_in(cwd: &Path) -> Command {
    let mut cmd = Command::cargo_bin("patchloom").unwrap();
    cmd.arg("--cwd").arg(cwd);
    cmd
}

fn write_fixture_file(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn seed_docs_smoke_fixture(root: &Path) {
    write_fixture_file(
        root,
        "Cargo.toml",
        "[package]\nname = \"smoke-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write_fixture_file(
        root,
        "src/lib.rs",
        "pub mod write;\n\n// TODO: rename old_function\n// TODO: update docs\npub fn old_function() -> &'static str {\n    \"old\"\n}\n",
    );
    write_fixture_file(
        root,
        "src/write.rs",
        "pub fn existing_write() -> &'static str {\n    \"write\"\n}\n",
    );
    write_fixture_file(
        root,
        "src/rename.rs",
        "pub fn old_name() -> &'static str {\n    \"old_name\"\n}\n",
    );
    write_fixture_file(
        root,
        "README.md",
        "# Smoke Fixture\n\nCurrent release: v1.0.0\nDocs version: v0.1.0\n\n## Commands\n\n| Command | Description |\n|---|---|\n| `search` | Search repo |\n",
    );
    write_fixture_file(
        root,
        "CHANGELOG.md",
        "# Changelog\n\n## Unreleased\n\n- Existing entry\n\n## 0.1.0\n\n- Initial release\n",
    );
    write_fixture_file(
        root,
        "AGENTS.md",
        "# AGENTS.md\n\n## Safety rules\n\n- existing rule\n\n## Safety rules\n\n- duplicate rule\n\n## FAQ\n\nQ: How?\nA: Like this.\n\n## License\n\nMIT\n",
    );
    write_fixture_file(
        root,
        "package.json",
        "{\n  \"name\": \"smoke-fixture\",\n  \"version\": \"1.0.0\"\n}\n",
    );
    write_fixture_file(
        root,
        "config.json",
        "{\n  \"database\": {\n    \"host\": \"localhost\",\n    \"port\": 3306\n  },\n  \"allowed_origins\": [\"http://localhost:3000\"],\n  \"deprecated_field\": true\n}\n",
    );
    write_fixture_file(
        root,
        "config.yaml",
        "users:\n  - name: alice\n    active: true\n  - name: bob\n    active: false\n",
    );
}

fn extract_markdown_code_block_after(markdown: &str, marker: &str, language: &str) -> String {
    let (_, after_marker) = markdown
        .split_once(marker)
        .expect("marker should exist in markdown");
    let fence = format!("```{language}\n");
    let (_, after_fence) = after_marker
        .split_once(&fence)
        .expect("fenced code block should exist after marker");
    let (block, _) = after_fence
        .split_once("\n```")
        .expect("fenced code block should terminate");
    block.to_string()
}

fn reference_path() -> PathBuf {
    repo_root().join("docs").join("reference").join("README.md")
}

fn extract_braced_block(source: &str, anchor: &str) -> String {
    let anchor_start = source
        .find(anchor)
        .unwrap_or_else(|| panic!("anchor `{anchor}` should exist"));
    let body_start = source[anchor_start..]
        .find('{')
        .map(|offset| anchor_start + offset)
        .expect("anchor should be followed by `{`");

    let mut depth = 0usize;
    for (offset, ch) in source[body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return source[body_start + 1..body_start + offset].to_string();
                }
            }
            _ => {}
        }
    }

    panic!("anchor `{anchor}` should have a matching closing brace");
}

fn read_anchored_block(path: &Path, anchor: &str) -> String {
    let source = fs::read_to_string(path).unwrap();
    extract_braced_block(&source, anchor)
}

fn camel_to_kebab(name: &str) -> String {
    let mut out = String::new();
    for (idx, ch) in name.chars().enumerate() {
        if ch.is_uppercase() {
            if idx > 0 {
                out.push('-');
            }
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn snake_to_kebab(name: &str) -> String {
    name.replace('_', "-")
}

fn collect_enum_variant_cli_names(path: &Path, anchor: &str) -> Vec<String> {
    let block = read_anchored_block(path, anchor);
    let re = regex::Regex::new(r"(?m)^\s*([A-Z][A-Za-z0-9]*)(?:\s*\(|\s*\{|,).*$").unwrap();
    re.captures_iter(&block)
        .map(|caps| camel_to_kebab(&caps[1]))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn collect_struct_field_names(path: &Path, anchor: &str) -> Vec<String> {
    let block = read_anchored_block(path, anchor);
    let re = regex::Regex::new(r"(?m)^\s*pub\s+([a-z_]+)\s*:").unwrap();
    re.captures_iter(&block)
        .map(|caps| caps[1].to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn collect_serde_rename_values(path: &Path, anchor: &str) -> Vec<String> {
    let block = read_anchored_block(path, anchor);
    // Match only variant-level renames (which always have an alias), not field-level renames.
    let re = regex::Regex::new(r#"#\[serde\(rename = "([^"]+)",\s*alias"#).unwrap();
    re.captures_iter(&block)
        .map(|caps| caps[1].to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn collect_batch_operation_names(path: &Path) -> Vec<String> {
    let block = read_anchored_block(path, "match op");
    let re = regex::Regex::new(r#"(?m)^\s*"([^"]+)"\s*=>"#).unwrap();
    re.captures_iter(&block)
        .map(|caps| caps[1].to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn collect_source_ref_markers(path: &Path) -> Vec<String> {
    let source = fs::read_to_string(path).unwrap();
    let re = regex::Regex::new(r"(?m)^\s*//\s*ref:([a-z0-9._:-]+)\s*$").unwrap();
    re.captures_iter(&source)
        .map(|caps| caps[1].to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn expected_reference_markers() -> Vec<String> {
    let root = repo_root();
    let cmd_dir = root.join("src").join("cmd");
    let global_path = root.join("src").join("cli").join("global.rs");
    let plan_path = root.join("src").join("plan").join("mod.rs");

    let mut markers = std::collections::BTreeSet::new();

    for name in collect_enum_variant_cli_names(&cmd_dir.join("mod.rs"), "pub enum Command") {
        markers.insert(format!("command:{name}"));
    }
    for name in collect_enum_variant_cli_names(&cmd_dir.join("doc.rs"), "pub enum DocAction") {
        markers.insert(format!("doc-action:{name}"));
    }
    for name in collect_enum_variant_cli_names(&cmd_dir.join("md.rs"), "pub enum MdAction") {
        markers.insert(format!("md-action:{name}"));
    }
    for name in collect_enum_variant_cli_names(&cmd_dir.join("patch.rs"), "pub enum PatchAction") {
        markers.insert(format!("patch-action:{name}"));
    }
    for name in collect_enum_variant_cli_names(&cmd_dir.join("tidy.rs"), "pub enum TidyAction") {
        markers.insert(format!("tidy-action:{name}"));
    }
    for name in collect_struct_field_names(&plan_path, "pub struct Plan") {
        markers.insert(format!("tx-field:{name}"));
    }
    for name in collect_serde_rename_values(&plan_path, "pub enum Operation") {
        markers.insert(format!("tx-op:{name}"));
    }

    let write_flags: std::collections::BTreeSet<String> =
        collect_struct_field_names(&global_path, "pub struct WriteFlags")
            .into_iter()
            .collect();
    for name in &write_flags {
        markers.insert(format!("write-flag:{}", snake_to_kebab(name)));
    }
    for name in collect_struct_field_names(&global_path, "pub struct GlobalFlags") {
        if !write_flags.contains(&name) {
            markers.insert(format!("global-flag:{}", snake_to_kebab(&name)));
        }
    }

    for entry in fs::read_dir(&cmd_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }

        for marker in collect_source_ref_markers(&path) {
            markers.insert(marker);
        }
    }

    markers.into_iter().collect()
}

fn reference_markers(reference: &str) -> Vec<(String, usize)> {
    let re = regex::Regex::new(r#"<!--\s*ref:([a-z0-9._:-]+)\s*-->"#).unwrap();
    re.captures_iter(reference)
        .map(|caps| (caps[1].to_string(), caps.get(0).unwrap().start()))
        .collect()
}

fn reference_section<'a>(reference: &'a str, markers: &[(String, usize)], marker: &str) -> &'a str {
    let idx = markers
        .iter()
        .position(|(name, _)| name == marker)
        .unwrap_or_else(|| panic!("marker `{marker}` should exist"));
    let start = markers[idx].1;
    let end = markers
        .get(idx + 1)
        .map(|(_, pos)| *pos)
        .unwrap_or(reference.len());
    &reference[start..end]
}

fn reference_doc_validation_errors(reference: &str) -> Vec<String> {
    let markers = reference_markers(reference);
    let actual_marker_names: std::collections::BTreeSet<String> =
        markers.iter().map(|(name, _)| name.clone()).collect();
    let expected_markers = expected_reference_markers();
    let missing_markers: Vec<String> = expected_markers
        .iter()
        .filter(|marker| !actual_marker_names.contains(*marker))
        .cloned()
        .collect();
    let mut errors = Vec::new();

    if !missing_markers.is_empty() {
        errors.push(format!(
            "reference doc is missing markers for:\n{}",
            missing_markers.join("\n")
        ));
    }

    for marker in expected_markers {
        if !actual_marker_names.contains(&marker) {
            continue;
        }

        let section = reference_section(reference, &markers, &marker);
        if !section.contains("**Use when:**") {
            errors.push(format!(
                "reference section `{marker}` must include a `Use when` stanza"
            ));
        }
    }

    errors
}

fn reference_without_use_when(reference: &str, marker: &str) -> String {
    let markers = reference_markers(reference);
    let section = reference_section(reference, &markers, marker);
    let use_when_start = section
        .find("- **Use when:**")
        .unwrap_or_else(|| panic!("reference section `{marker}` should include `Use when`"));
    let use_when_end = section[use_when_start..]
        .find('\n')
        .map(|offset| use_when_start + offset + 1)
        .unwrap_or(section.len());
    let broken_section = format!("{}{}", &section[..use_when_start], &section[use_when_end..]);

    reference.replacen(section, &broken_section, 1)
}

fn has_mcp_support() -> bool {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("mcp-server")
        .arg("--help")
        .ok()
        .is_ok()
}

#[cfg(feature = "mcp-http")]
fn has_mcp_http_support() -> bool {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("mcp-server")
        .arg("--help")
        .ok()
        .expect("mcp-server --help failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.contains("--http")
}

/// Spawn `patchloom mcp-server` in a tempdir and return a connected MCP client.
async fn spawn_mcp_client(cwd: &Path) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    use rmcp::ServiceExt;
    use rmcp::transport::TokioChildProcess;

    let bin = assert_cmd::cargo::cargo_bin("patchloom");
    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("mcp-server").current_dir(cwd);

    let transport = TokioChildProcess::new(cmd).expect("failed to spawn patchloom mcp-server");
    ().serve(transport)
        .await
        .expect("failed to connect MCP client")
}

/// Helper: call a tool and return the text content from the first Content item.
async fn call_tool_text(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tool: impl Into<String>,
    args: serde_json::Value,
) -> (bool, String) {
    let params = rmcp::model::CallToolRequestParams::new(tool.into())
        .with_arguments(serde_json::from_value(args).unwrap());
    let result = match client.peer().call_tool(params).await {
        Ok(r) => r,
        Err(e) => {
            return (true, format!("MCP call error: {e}"));
        }
    };
    let is_error = result.is_error.unwrap_or(false);
    let text = result
        .content
        .first()
        .and_then(|c| match c {
            rmcp::model::ContentBlock::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .unwrap_or_default();
    (is_error, text)
}

/// Helper to call an MCP tool and parse the response text as JSON Value.
/// This enables honest structured assertions for tools like git_status
/// (instead of fragile substring `contains` on the whole pretty-printed output).
async fn call_tool_value(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tool: impl Into<String>,
    args: serde_json::Value,
) -> (bool, serde_json::Value) {
    let (is_error, text) = call_tool_text(client, tool, args).await;
    let val =
        serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({ "raw_text": text }));
    (is_error, val)
}

// ---------------------------------------------------------------------------
// Backup pruning integration tests (#371)
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn readonly_dir_blocks_writes(dir: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(dir, fs::Permissions::from_mode(0o555)).unwrap();
    let probe = dir.join(".write_probe");
    match fs::write(&probe, "x") {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
            false
        }
        Err(_) => true,
    }
}

mod agent_rules_tests;
mod ast_tests;
mod batch_tests;
mod cli_flags_tests;
mod create_tests;
mod delete_tests;
mod doc_tests;
mod explain_tests;
mod file_ops_tests;
mod init_tests;
mod mcp_tests;
mod mcp_tool_tests;
mod md_tests;
mod misc_tests;
mod parse_tests;
mod patch_tests;
mod read_tests;
mod reference_docs_tests;
mod rename_tests;
mod replace_tests;
mod schema_tests;
mod search_tests;
mod smoke_tests;
mod status_tests;
mod tidy_tests;
mod tx_tests;
mod undo_tests;
mod write_path_contract_tests;
