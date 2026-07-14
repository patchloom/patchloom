use super::*;

#[test]
fn test_ci_workflow_routes_macos_fork_prs_to_github_hosted_runners() {
    let ci = fs::read_to_string(ci_workflow_path()).unwrap();

    // Both macOS jobs (ci-macos, msrv-macos) should use GitHub-hosted runners
    let macos_runs_on = "runs-on: macos-latest";
    assert_eq!(
        ci.matches(macos_runs_on).count(),
        2,
        "ci.yml should have exactly 2 macOS jobs on macos-latest"
    );
}

#[test]
fn test_ci_workflow_uses_runner_temp_for_bench_fixtures() {
    let ci = fs::read_to_string(ci_workflow_path()).unwrap();

    assert!(
        ci.contains("\"$RUNNER_TEMP/bench.json\"") || ci.contains("$RUNNER_TEMP/bench.json"),
        "ci.yml should keep benchmark JSON fixtures under RUNNER_TEMP"
    );
    assert!(
        ci.contains("\"$RUNNER_TEMP/bench.txt\"") || ci.contains("$RUNNER_TEMP/bench.txt"),
        "ci.yml should keep benchmark text fixtures under RUNNER_TEMP"
    );
    assert!(!ci.contains("/tmp/bench.json"));
    assert!(!ci.contains("/tmp/bench.txt"));
}

#[test]
fn test_ci_bench_steps_use_shared_threshold_script() {
    let ci = fs::read_to_string(ci_workflow_path()).unwrap();
    assert!(
        ci.contains("benches/ci/check_threshold.py"),
        "ci.yml bench steps should call the shared check_threshold.py script"
    );
    assert!(
        !ci.contains("python3 - \"$bench_json\""),
        "ci.yml should not inline the threshold Python snippet"
    );
    assert!(
        repo_root().join("benches/ci/check_threshold.py").exists(),
        "benches/ci/check_threshold.py must exist on disk"
    );
}

#[test]
fn test_bench_workflow_limits_artifact_retention() {
    let bench = fs::read_to_string(repo_root().join(".github/workflows/bench.yml")).unwrap();
    assert!(
        bench.contains("retention-days: 14"),
        "benchmark artifacts should use a short explicit retention period"
    );
}

#[test]
fn test_bench_workflow_passes_dispatch_scales_via_env() {
    let bench = fs::read_to_string(repo_root().join(".github/workflows/bench.yml")).unwrap();
    assert!(
        bench.contains("BENCH_SCALES: ${{ inputs.scales || 'small medium' }}"),
        "bench workflow should pass dispatch scales through an environment variable"
    );
    assert!(
        bench.contains("bash run.sh \"$BENCH_SCALES\""),
        "bench workflow should quote BENCH_SCALES when invoking the benchmark runner"
    );
    assert!(
        !bench.contains("bash run.sh ${{ inputs.scales || 'small medium' }}"),
        "bench workflow should not interpolate workflow inputs directly into the shell command"
    );
}

#[test]
fn test_reference_doc_covers_meaningful_feature_inventory() {
    let reference = fs::read_to_string(reference_path()).unwrap();
    let errors = reference_doc_validation_errors(&reference);

    assert!(errors.is_empty(), "{}", errors.join("\n\n"));
}

#[test]
fn test_reference_doc_describes_status_and_undo_contracts() {
    let reference = fs::read_to_string(reference_path()).unwrap();
    assert!(
        reference.contains("This command is git-backed, so it must run inside a git repository.")
    );
    // Undo dry-run singularity (fixrealloop agent trap): bare undo previews only.
    assert!(
        reference.contains("**Default is dry-run**")
            && reference.contains("exits `2` (`CHANGES_DETECTED`)")
            && reference.contains("Pass `--apply` to restore"),
        "reference must document undo dry-run exit 2 and --apply restore"
    );
}

#[test]
fn test_reference_doc_describes_confirm_json_applied_contract_for_file_lifecycle_commands() {
    let reference = fs::read_to_string(reference_path()).unwrap();
    let contract = "When combined with `--confirm` and `--json` or `--jsonl`, the structured output includes `applied: true|false` so callers can tell whether the prompt was accepted.";
    assert_eq!(reference.matches(contract).count(), 3);
}

#[test]
fn test_reference_doc_describes_tx_lifecycle_failure_context() {
    let reference = fs::read_to_string(reference_path()).unwrap();
    assert!(reference.contains(
        "Error output reports the failing step number, exit status, the lifecycle working directory (`cwd`), and a truncated snippet of the command's stderr when available."
    ));
}

#[test]
fn test_reference_doc_describes_create_input_contract() {
    let reference = fs::read_to_string(reference_path()).unwrap();
    assert!(reference.contains("Exactly one of `--content` or `--stdin` is required."));
    assert!(
        reference
            .contains("Passing both is rejected with `--content and --stdin cannot be combined`")
    );
    assert!(reference.contains(
        "passing neither is rejected with `either --content or --stdin must be provided`"
    ));
}

#[test]
fn test_reference_doc_requires_use_when_stanza() {
    let reference = fs::read_to_string(reference_path()).unwrap();
    let broken = reference_without_use_when(&reference, "patch-mode:file");
    let errors = reference_doc_validation_errors(&broken);

    assert!(
        errors.iter().any(|error| error
            .contains("reference section `patch-mode:file` must include a `Use when` stanza")),
        "expected missing `Use when` error, got:\n{}",
        errors.join("\n\n")
    );
}
