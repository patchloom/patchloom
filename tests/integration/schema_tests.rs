use super::*;

#[test]
fn test_schema_json_output() {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["schema", "--format", "json"])
        .assert()
        .code(0);

    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    // Envelope must contain version, operations, and plan_envelope.
    let version = parsed
        .get("version")
        .and_then(|v| v.as_str())
        .expect("version must be a non-empty string");
    assert!(!version.is_empty(), "version must not be empty");
    let ops = parsed
        .get("operations")
        .and_then(|v| v.as_array())
        .expect("operations must be an array");
    assert!(
        !ops.is_empty(),
        "schema should return at least one operation"
    );

    // Every operation must have name, description, parameters, min_tier.
    for op in ops {
        let name = op
            .get("name")
            .and_then(|v| v.as_str())
            .expect("name must be a non-empty string in schema entry");
        assert!(!name.is_empty(), "name must not be empty in schema entry");
        let desc = op
            .get("description")
            .and_then(|v| v.as_str())
            .expect("description must be a non-empty string in schema entry");
        assert!(
            !desc.is_empty(),
            "description must not be empty for op {name}"
        );
        assert!(
            op.get("parameters").and_then(|v| v.as_object()).is_some(),
            "parameters must be an object for op {name}"
        );
        assert!(
            op.get("min_tier").and_then(|v| v.as_str()).is_some(),
            "min_tier must be a string for op {name}"
        );
    }

    // Plan envelope must include write_policy with all fields as schema objects.
    let wp = parsed
        .pointer("/plan_envelope/write_policy/properties")
        .expect("plan_envelope.write_policy.properties must exist");
    for key in [
        "ensure_final_newline",
        "normalize_eol",
        "trim_trailing_whitespace",
        "collapse_blanks",
    ] {
        assert!(
            wp.get(key).and_then(|v| v.as_object()).is_some(),
            "write_policy property {key} must be a schema object"
        );
    }
}

#[test]
fn test_schema_prompt_output() {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["schema", "--format", "prompt"])
        .assert()
        .code(0);

    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("# Patchloom Operations"));
    assert!(stdout.contains("format version:"));
    assert!(stdout.contains("replace"));
}

#[test]
fn test_schema_tier_filtering() {
    let weak_output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["schema", "--format", "json", "--tier", "weak"])
        .assert()
        .code(0);

    let strong_output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["schema", "--format", "json", "--tier", "strong"])
        .assert()
        .code(0);

    let weak_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8(weak_output.get_output().stdout.clone()).unwrap())
            .unwrap();
    let strong_json: serde_json::Value = serde_json::from_str(
        &String::from_utf8(strong_output.get_output().stdout.clone()).unwrap(),
    )
    .unwrap();

    let weak = weak_json
        .get("operations")
        .and_then(|v| v.as_array())
        .expect("weak operations must be an array");
    let strong = strong_json
        .get("operations")
        .and_then(|v| v.as_array())
        .expect("strong operations must be an array");

    assert!(
        weak.len() < strong.len(),
        "weak tier ({}) should have fewer ops than strong ({})",
        weak.len(),
        strong.len()
    );
    // All weak ops should have min_tier "weak".
    for op in weak {
        assert_eq!(
            op.get("min_tier").and_then(|t| t.as_str()),
            Some("weak"),
            "weak tier should only contain weak ops"
        );
    }
}

#[test]
fn test_schema_examples_flag() {
    // Without --examples, entries should not have "examples" key.
    let without_output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["schema", "--format", "json"])
        .assert()
        .code(0);

    let without_json: serde_json::Value = serde_json::from_str(
        &String::from_utf8(without_output.get_output().stdout.clone()).unwrap(),
    )
    .unwrap();
    let without = without_json
        .get("operations")
        .and_then(|v| v.as_array())
        .expect("operations must be an array");
    for op in without {
        assert!(
            op.get("examples").is_none(),
            "examples should be stripped without --examples flag"
        );
    }

    // With --examples, entries should have "examples" key.
    let with_output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["schema", "--format", "json", "--examples"])
        .assert()
        .code(0);

    let with_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8(with_output.get_output().stdout.clone()).unwrap())
            .unwrap();
    let with = with_json
        .get("operations")
        .and_then(|v| v.as_array())
        .expect("operations must be an array");
    assert!(
        with.iter()
            .any(|op| op.get("examples").and_then(|v| v.as_array()).is_some()),
        "at least one op should have examples (as an array) with --examples flag"
    );
}

#[test]
fn test_schema_quiet_suppresses_output() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["schema", "--format", "json", "--quiet"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn test_schema_invalid_tier_fails() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["schema", "--tier", "invalid"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("unknown tier"));
}

// ---------------------------------------------------------------------------
// md --check produces stdout output (#544)
// ---------------------------------------------------------------------------
