use super::*;

#[test]
fn test_multi_glob_filters_multiple_patterns() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.rs"), "match\n").unwrap();
    fs::write(dir.path().join("b.md"), "match\n").unwrap();
    fs::write(dir.path().join("c.txt"), "match\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("match")
        .arg("--glob")
        .arg("*.rs")
        .arg("--glob")
        .arg("*.md")
        .arg(dir.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("a.rs"));
    assert!(stdout.contains("b.md"));
    assert!(!stdout.contains("c.txt"));
}

#[test]
fn test_launch_announcement_command_count_matches_readme() {
    let readme = fs::read_to_string(readme_path()).unwrap();
    let launch = fs::read_to_string(launch_announcement_path()).unwrap();

    // Extract command count from README (e.g., "across 19 commands").
    let readme_count: usize = readme
        .lines()
        .find(|l| l.contains("tests across") && l.contains("commands"))
        .and_then(|l| {
            let idx = l.find("commands")?;
            l[..idx]
                .split_whitespace()
                .next_back()?
                .parse::<usize>()
                .ok()
        })
        .expect("README should state the command count");

    // Launch announcement says "N commands cover".
    assert!(
        launch.contains(&format!("{readme_count} commands cover")),
        "launch announcement command count should match README ({readme_count} commands)"
    );
}

#[test]
fn test_agent_test_readme_uses_virtualenv_for_direct_install() {
    let content = fs::read_to_string(agent_test_readme_path()).unwrap();
    assert!(
        content.contains("python3 -m venv .venv"),
        "agent test readme should create a virtualenv before pip install"
    );
    assert!(
        content.contains(". .venv/bin/activate"),
        "agent test readme should show how to activate the virtualenv"
    );
}

#[test]
fn test_contributing_make_targets_table_covers_key_targets() {
    let contributing = fs::read_to_string(repo_root().join("CONTRIBUTING.md")).unwrap();
    for target in [
        "make check",
        "make check-fast",
        "make build",
        "make fmt",
        "make test",
        "make integration-test",
        "make clippy",
        "make update-readme",
        "cargo check --all-targets",
    ] {
        assert!(
            contributing.contains(&format!("| `{target}`")),
            "CONTRIBUTING.md make-targets table should list {target}"
        );
    }
}

#[test]
fn test_makefile_has_audit_target() {
    let makefile = fs::read_to_string(repo_root().join("Makefile")).unwrap();
    assert!(
        makefile.contains("audit:"),
        "Makefile should have an audit target for local vulnerability scanning"
    );
}

#[test]
fn test_workflows_disable_persisted_checkout_credentials_by_default() {
    let ci = fs::read_to_string(ci_workflow_path()).unwrap();
    assert!(ci.matches("persist-credentials: false").count() >= 8);

    let security = fs::read_to_string(repo_root().join(".github/workflows/security.yml")).unwrap();
    assert_eq!(security.matches("persist-credentials: false").count(), 4);

    let bench = fs::read_to_string(repo_root().join(".github/workflows/bench.yml")).unwrap();
    assert_eq!(bench.matches("persist-credentials: false").count(), 1);
}

#[test]
fn test_publish_crates_workflow_serializes_publishes_per_ref() {
    let publish =
        fs::read_to_string(repo_root().join(".github/workflows/publish-crates.yml")).unwrap();
    assert!(
        publish.contains("group: publish-crates-${{ github.ref }}"),
        "publish-crates workflow should serialize publishes per ref"
    );
    assert!(
        publish.contains("cancel-in-progress: false"),
        "publish-crates workflow should queue duplicate publishes instead of cancelling them"
    );
}

#[test]
fn test_readme_agent_test_count_matches_non_benchmark_scenarios() {
    let count = fs::read_dir(repo_root().join("tests/agent"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("test_") && name.ends_with(".py") && name != "test_bench.py"
        })
        .map(|entry| fs::read_to_string(entry.path()).unwrap())
        .map(|content| {
            content
                .lines()
                .filter(|line| line.trim_start().starts_with("def test_"))
                .count()
        })
        .sum::<usize>();

    let readme = fs::read_to_string(readme_path()).unwrap();
    assert!(
        readme.contains(&format!("`make agent-test` runs {count} pytest scenarios")),
        "README should document the non-benchmark agent scenario count run by make agent-test"
    );
}

#[test]
fn test_concepts_doc_mentions_confirm_write_mode() {
    let concepts = fs::read_to_string(concepts_path()).unwrap();
    assert!(concepts.contains("Every write command supports four modes:"));
    assert!(concepts.contains("| `--confirm` | Show the diff, then prompt before writing |"));
}

#[test]
fn test_quickstart_doc_describes_tx_lifecycle_failure_context() {
    let quickstart = fs::read_to_string(quickstart_path()).unwrap();
    assert!(quickstart.contains("Lifecycle failure output includes the failing step"));
    assert!(quickstart.contains("status, and the `cwd` used for that step."));
}

#[test]
fn test_agent_driver_subcommands_match_cli() {
    // Get subcommands from `patchloom --help` output.
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--help")
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&output.stdout);
    let mut cli_commands: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut in_commands = false;
    for line in help.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Commands:") {
            in_commands = true;
            continue;
        }
        if in_commands {
            if trimmed.is_empty() || trimmed.starts_with("Options:") {
                break;
            }
            if let Some(cmd) = trimmed.split_whitespace().next() {
                // Skip "help" -- it's generated by clap, not a real subcommand.
                if cmd != "help" {
                    cli_commands.insert(cmd.to_string());
                }
            }
        }
    }

    // Read the Python file and extract the set literal.
    let base_py = fs::read_to_string("tests/agent/drivers/base.py").unwrap();
    let mut py_commands: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut in_set = false;
    for line in base_py.lines() {
        if line.contains("_PATCHLOOM_SUBCOMMANDS") && line.contains('{') {
            in_set = true;
        }
        if in_set {
            // Extract quoted strings from the line.
            for part in line.split('"') {
                // Every other split segment is inside quotes.
                // We collect segments that look like command names.
                if !part.is_empty()
                    && !part.contains('{')
                    && !part.contains('}')
                    && !part.contains(',')
                    && !part.contains(' ')
                    && !part.contains('=')
                    && !part.contains('#')
                {
                    py_commands.insert(part.to_string());
                }
            }
            if line.contains('}') {
                break;
            }
        }
    }

    // mcp-server is feature-gated and may not appear in --help without
    // the mcp feature, but should be in the Python set. Remove it from
    // the Python set for comparison if it's not in CLI output.
    if !cli_commands.contains("mcp-server") {
        py_commands.remove("mcp-server");
    }

    let missing_from_py: Vec<_> = cli_commands.difference(&py_commands).collect();
    let extra_in_py: Vec<_> = py_commands.difference(&cli_commands).collect();

    assert!(
        missing_from_py.is_empty() && extra_in_py.is_empty(),
        "_PATCHLOOM_SUBCOMMANDS is out of sync with CLI.\n\
         Missing from Python set: {:?}\n\
         Extra in Python set: {:?}",
        missing_from_py,
        extra_in_py,
    );
}

// ---------------------------------------------------------------------------
// Verbose flag
// ---------------------------------------------------------------------------
