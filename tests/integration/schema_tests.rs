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

    // Envelope must contain ok, version, operations, and plan_envelope.
    assert_eq!(parsed["ok"], true);
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
    // clap ValueEnum rejects free strings at parse time. Mapped to FAILURE (1)
    // so it does not collide with CHANGES_DETECTED (2).
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["schema", "--tier", "invalid"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("possible values"))
        .stderr(predicate::str::contains("weak"));
}

#[test]
fn test_schema_tier_small_is_not_accepted() {
    // Industry model-size prior; product tiers are weak/medium/strong only.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["schema", "--tier", "small"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("possible values"));
}

#[test]
fn test_schema_help_lists_tier_enum() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["schema", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Possible values"))
        .stdout(predicate::str::contains("weak"))
        .stdout(predicate::str::contains("medium"))
        .stdout(predicate::str::contains("strong"));
}

// ---------------------------------------------------------------------------
// #1289: Operation field descriptions in schema
// ---------------------------------------------------------------------------

#[test]
fn test_schema_operation_fields_have_descriptions() {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["schema", "--format", "json"])
        .assert()
        .code(0);

    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let ops = parsed["operations"].as_array().unwrap();

    // Spot-check key operations that must have field descriptions.
    let checks: &[(&str, &[&str])] = &[
        ("doc.set", &["path", "selector", "value"]),
        ("doc.delete", &["path", "selector"]),
        ("replace", &["old", "new"]),
    ];

    for (op_name, required_fields) in checks {
        let op = ops
            .iter()
            .find(|o| o["name"].as_str() == Some(op_name))
            .unwrap_or_else(|| panic!("operation '{op_name}' not found in schema"));
        let props = op["parameters"]["properties"]
            .as_object()
            .unwrap_or_else(|| panic!("no properties for {op_name}"));

        for field in *required_fields {
            let field_schema = props
                .get(*field)
                .unwrap_or_else(|| panic!("field '{field}' missing from {op_name} parameters"));
            let desc = field_schema.get("description").and_then(|d| d.as_str());
            assert!(
                desc.is_some_and(|d| !d.is_empty()),
                "field '{field}' in operation '{op_name}' must have a non-empty description"
            );
        }
    }
}

#[test]
#[cfg(feature = "ast")]
fn test_schema_ast_rename_path_description_mentions_directory() {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["schema", "--format", "json"])
        .assert()
        .code(0);

    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let ops = parsed["operations"].as_array().unwrap();

    let ast_rename = ops
        .iter()
        .find(|o| o["name"].as_str() == Some("ast.rename"))
        .expect("ast.rename must be in schema");
    let path_desc = ast_rename
        .pointer("/parameters/properties/path/description")
        .and_then(|d| d.as_str())
        .expect("ast.rename path field must have a description");
    assert!(
        path_desc.to_lowercase().contains("director"),
        "ast.rename path description should mention directory support: {path_desc}"
    );
}

// ---------------------------------------------------------------------------
// md --check produces stdout output (#544)
// ---------------------------------------------------------------------------
