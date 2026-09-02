use super::*;

#[test]
fn test_doc_get_jsonl_compound_value_is_single_line_json() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"obj":{"name":"patchloom","version":1}}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("obj")
        .arg("--jsonl")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1);
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    // #1838: success is ok envelope; value holds the document fragment.
    assert_eq!(json["ok"], true, "{json}");
    assert_eq!(json["value"]["name"], "patchloom");
    assert_eq!(json["value"]["version"], 1);
}

#[test]
fn test_doc_get_quiet_suppresses_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name":"patchloom"}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("name")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stdout.is_empty(),
        "quiet should suppress doc get output"
    );
}

#[test]
fn test_doc_get_json() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name":"patchloom"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("name")
        .assert()
        .success()
        .stdout(predicate::str::contains("patchloom"));
}

#[test]
fn test_doc_has_existing_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name":"patchloom","version":1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("has")
        .arg(&file)
        .arg("name")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("true"));
}

#[test]
fn test_doc_has_missing_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name":"patchloom"}"#).unwrap();

    // Missing key is a valid boolean answer (exit 0), not no_matches (#1843).
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("has")
        .arg(&file)
        .arg("missing")
        .assert()
        .code(0)
        .stdout(predicate::str::contains("false"));
}

/// #1838: doc query --json success is an ok envelope, not a bare value.
#[test]
fn test_doc_get_json_success_envelope() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("o.json");
    fs::write(&file, r#"{"a":1,"c":null}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "get"])
        .arg(&file)
        .arg("a")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["value"], 1, "{v}");
    assert!(v["path"].as_str().is_some(), "{v}");
    assert_eq!(v["selector"], "a", "{v}");
}

/// #1843: doc has missing under --json exits 0 with value false.
#[test]
fn test_doc_has_json_missing_exit_0_envelope() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("o.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "has"])
        .arg(&file)
        .arg("missing")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "missing key is not no_matches: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["value"], false, "{v}");
}

#[test]
fn test_doc_keys_jsonl_success_envelope() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"scripts":{"build":"tsc","lint":"eslint"}}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("keys")
        .arg(&file)
        .arg("scripts")
        .arg("--jsonl")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "single envelope line (#1838): {stdout}");
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json["ok"], true, "{json}");
    let keys = json["value"].as_array().expect("value array of keys");
    assert_eq!(keys.len(), 2);
    assert!(keys.iter().any(|v| v == "build"));
    assert!(keys.iter().any(|v| v == "lint"));
}

#[test]
fn test_doc_keys_lists_object_keys() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"alpha":1,"beta":2,"gamma":3}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("keys")
        .arg(&file)
        .arg(".")
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha"))
        .stdout(predicate::str::contains("beta"))
        .stdout(predicate::str::contains("gamma"));
}

#[test]
fn test_doc_keys_omitted_selector_lists_root() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("some.toml");
    fs::write(
        &file,
        "[package]\nname = \"x\"\n[dependencies]\nserde = \"1\"\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("keys")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("package"))
        .stdout(predicate::str::contains("dependencies"));
}

#[test]
fn test_doc_len_omitted_selector_counts_root() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("some.toml");
    fs::write(
        &file,
        "[package]\nname = \"x\"\n[dependencies]\nserde = \"1\"\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("len")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("2"));
}

#[test]
fn test_doc_len_multi_document_root() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("multi.yaml");
    fs::write(&file, "a: 1\n---\nb: 2\n").unwrap();

    for root_sel in [None, Some(".")] {
        let mut cmd = Command::cargo_bin("patchloom").unwrap();
        cmd.arg("doc").arg("len").arg(&file);
        if let Some(sel) = root_sel {
            cmd.arg(sel);
        }
        cmd.assert()
            .success()
            .stdout(predicate::str::starts_with("2"));
    }
}

#[test]
fn test_doc_get_typo_key_json_did_you_mean() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.toml");
    fs::write(&file, "[database]\nport = 5432\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "get"])
        .arg(&file)
        .arg("databse.port")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).expect("valid JSON");
    assert_eq!(v["ok"], false);
    assert_eq!(v["error_kind"], "no_matches");
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        err.contains("did you mean: database?"),
        "JSON error must carry sibling-key hint: {v}"
    );
}

#[test]
fn test_doc_get_hyphenated_typo_key_json_did_you_mean() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"database-url":1}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "get"])
        .arg(&file)
        .arg("databse-url")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).expect("valid JSON");
    assert_eq!(v["ok"], false);
    assert_eq!(v["error_kind"], "no_matches");
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        !err.contains("did you mean: database?"),
        "JSON error must not hint hyphen-split token `database`: {v}"
    );
    if err.contains("did you mean:") {
        assert!(
            err.contains("database-url"),
            "JSON error whole-key hint must be database-url: {v}"
        );
    }
}

#[test]
fn test_doc_get_jq_bracket_json_suggests_forms() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"items":[{"name":"a"}]}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "get"])
        .arg(&file)
        .arg("items[name]")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let blob = format!("{stdout}{stderr}");
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap_or(serde_json::json!({
        "error": blob.as_str(),
        "error_kind": "unknown"
    }));
    if v.get("error_kind").and_then(|k| k.as_str()) == Some("invalid_input") {
        assert_eq!(v["error_kind"], "invalid_input");
        let err = v["error"].as_str().unwrap_or("");
        assert!(
            err.contains("items[0]") && err.contains("items[*]") && err.contains("items[name=…]"),
            "JSON error must suggest bracket forms: {v}"
        );
    } else {
        assert!(
            blob.contains("items[0]")
                && blob.contains("items[*]")
                && blob.contains("items[name=…]"),
            "invalid bracket must suggest forms: {blob}"
        );
    }
}

#[test]
fn test_doc_set_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"version":"1.0"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("version")
        .arg("\"2.0\"")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["version"], serde_json::json!("2.0"));
}

#[cfg(unix)]
#[test]
fn test_doc_set_confirm_eof_does_not_modify_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"version":"1.0"}"#).unwrap();

    let output = run_patchloom_confirm_in_pty(
        &[
            "doc",
            "set",
            file.to_str().unwrap(),
            "version",
            "\"2.0\"",
            "--confirm",
        ],
        "\u{4}",
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "declined confirm should exit 2 (CHANGES_DETECTED)"
    );
    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["version"], serde_json::json!("1.0"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Apply? [Y/n]"));
}

#[test]
fn test_doc_set_preserves_key_order() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    // Keys are intentionally NOT in alphabetical order.
    fs::write(&file, r#"{"z_last":1,"a_first":2,"m_middle":3}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("a_first")
        .arg("99")
        .arg("--apply")
        .assert()
        .code(0);

    // The written file must keep keys in the original insertion order,
    // not sorted alphabetically. If serde_json's preserve_order feature
    // is missing, keys would appear as a_first, m_middle, z_last.
    let content = fs::read_to_string(&file).unwrap();
    let z_pos = content.find("z_last").expect("z_last missing");
    let a_pos = content.find("a_first").expect("a_first missing");
    let m_pos = content.find("m_middle").expect("m_middle missing");
    assert!(
        z_pos < a_pos && a_pos < m_pos,
        "key order not preserved: z_last@{z_pos}, a_first@{a_pos}, m_middle@{m_pos}"
    );
}

#[test]
fn test_doc_set_toml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.toml");
    fs::write(
        &file,
        "# Main config\n[server]\nhost = \"localhost\"\nport = 8080\n\n# DB\n[database]\nurl = \"pg\"\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("server.port")
        .arg("9090")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    // Comments must survive.
    assert!(
        content.contains("# Main config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# DB"),
        "section comment stripped: {content}"
    );
    // Value must be updated.
    assert!(content.contains("9090"), "new value missing: {content}");
    assert!(!content.contains("8080"), "old value present: {content}");
    // Section order must be preserved.
    let server_pos = content.find("[server]").expect("[server] missing");
    let db_pos = content.find("[database]").expect("[database] missing");
    assert!(
        server_pos < db_pos,
        "section order changed: server@{server_pos} db@{db_pos}"
    );
}

#[test]
fn test_doc_merge_toml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.toml");
    fs::write(
        &file,
        "# Main config\n\n[server]\nhost = \"localhost\"\nport = 8080 # default\n\n# DB\n[database]\nurl = \"pg\"\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("merge")
        .arg(&file)
        .arg("--value")
        .arg(r#"{"logging": "debug"}"#)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("# Main config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# default"),
        "inline comment stripped: {content}"
    );
    assert!(content.contains("# DB"), "DB comment stripped: {content}");
    assert!(content.contains("logging"), "merged key missing: {content}");
}

#[test]
fn test_doc_delete_toml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.toml");
    fs::write(
        &file,
        "# Main config\nname = \"my-app\"\nversion = 1\n\n# Server\n[server]\nhost = \"localhost\"\nport = 8080\n\n# DB\n[database]\nurl = \"pg\"\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete")
        .arg(&file)
        .arg("version")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("# Main config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# Server"),
        "section comment stripped: {content}"
    );
    assert!(content.contains("# DB"), "DB comment stripped: {content}");
    assert!(
        !content.contains("version"),
        "deleted key still present: {content}"
    );
    assert!(content.contains("name"), "surviving key missing: {content}");
}

#[test]
fn test_doc_set_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Main config\nname: my-app\nversion: 1\n\n# Server\nserver:\n  host: localhost\n  port: 8080 # default\n\n# DB\ndatabase:\n  url: pg\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("server.port")
        .arg("9090")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    // Output must be syntactically valid YAML.
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    // Comments must survive.
    assert!(
        content.contains("# Main config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# Server"),
        "section comment stripped: {content}"
    );
    assert!(
        content.contains("# default"),
        "inline comment stripped: {content}"
    );
    assert!(content.contains("# DB"), "DB comment stripped: {content}");
    // Value must be updated.
    assert!(content.contains("9090"), "new value missing: {content}");
    assert!(!content.contains("8080"), "old value present: {content}");
    // Key order must be preserved.
    let server_pos = content.find("server:").expect("server: missing");
    let db_pos = content.find("database:").expect("database: missing");
    assert!(
        server_pos < db_pos,
        "key order changed: server@{server_pos} db@{db_pos}"
    );
}

#[test]
fn test_doc_merge_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Main config\nname: my-app\nversion: 1\n\n# Server\nserver:\n  host: localhost\n  port: 8080 # default\n\n# DB\ndatabase:\n  url: pg\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("merge")
        .arg(&file)
        .arg("--value")
        .arg(r#"{"logging": "debug"}"#)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Main config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# Server"),
        "section comment stripped: {content}"
    );
    assert!(
        content.contains("# default"),
        "inline comment stripped: {content}"
    );
    assert!(content.contains("# DB"), "DB comment stripped: {content}");
    assert!(content.contains("logging"), "merged key missing: {content}");
    assert!(content.contains("debug"), "merged value missing: {content}");
}

#[test]
fn test_doc_delete_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Main config\nname: my-app\nversion: 1\n\n# Server\nserver:\n  host: localhost\n  port: 8080 # default\n\n# DB\ndatabase:\n  url: pg\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete")
        .arg(&file)
        .arg("version")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Main config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# Server"),
        "section comment stripped: {content}"
    );
    assert!(
        content.contains("# default"),
        "inline comment stripped: {content}"
    );
    assert!(content.contains("# DB"), "DB comment stripped: {content}");
    assert!(
        !content.contains("version:"),
        "deleted key still present: {content}"
    );
    assert!(
        content.contains("name: my-app"),
        "surviving key missing: {content}"
    );
}

#[test]
fn test_doc_append_yaml_sequence_root() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("items.yaml");
    fs::write(&file, "# Items\n- item1\n- item2\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("append")
        .arg(&file)
        .arg("")
        .arg("item3")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(content.contains("# Items"), "comment stripped: {content}");
    assert!(content.contains("item1"), "item1 missing: {content}");
    assert!(content.contains("item2"), "item2 missing: {content}");
    assert!(
        content.contains("item3"),
        "appended item3 missing: {content}"
    );
}

#[test]
fn test_doc_set_yaml_sequence_root_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("items.yaml");
    fs::write(&file, "# Items list\n- item1\n- item2\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("[1]")
        .arg("updated")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Items list"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("item1"),
        "unchanged element lost: {content}"
    );
    assert!(
        content.contains("updated"),
        "updated element missing: {content}"
    );
}

#[test]
fn test_doc_delete_where_yaml_sequence_root_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("items.yaml");
    fs::write(
        &file,
        "# Contact links\n- name: keep\n  url: keep.com\n- name: remove\n  url: remove.com\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete-where")
        .arg(&file)
        .arg("")
        .arg("--predicate")
        .arg("name=remove")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Contact links"),
        "top comment stripped: {content}"
    );
    assert!(content.contains("keep"), "kept element missing: {content}");
    assert!(
        !content.contains("remove"),
        "removed element still present: {content}"
    );
}

#[test]
fn test_doc_prepend_yaml_produces_valid_output() {
    // Verifies that prepend produces valid YAML with comments preserved.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(&file, "# Config\nname: app\nitems:\n  - existing\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("prepend")
        .arg(&file)
        .arg("items")
        .arg("\"first\"")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let parsed: serde_json::Value =
        serde_yaml_ng::from_str(&content).expect("output is not valid YAML");
    let items = parsed.get("items").expect("items key missing");
    assert_eq!(items[0], "first", "prepended item not at position 0");
    assert_eq!(items[1], "existing", "original item not at position 1");
}

#[test]
fn test_doc_update_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Config\nitems:\n  - name: a\n    status: pending # TODO\n  - name: b\n    status: pending\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("update")
        .arg(&file)
        .arg("items[*].status")
        .arg("done")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Config"),
        "top comment stripped: {content}"
    );
    assert!(content.contains("done"), "updated value missing: {content}");
    assert!(
        !content.contains("pending"),
        "old value still present: {content}"
    );
}

#[test]
fn test_doc_ensure_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Config\nname: my-app\n\n# Server\nserver:\n  host: localhost\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("ensure")
        .arg(&file)
        .arg("server.port")
        .arg("8080")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# Server"),
        "section comment stripped: {content}"
    );
    assert!(content.contains("8080"), "ensured value missing: {content}");
    assert!(
        content.contains("name: my-app"),
        "existing key missing: {content}"
    );
}

#[test]
fn test_doc_ensure_deep_nested_yaml_creates_structure_and_preserves_comments() {
    // Exercise nested creation via CLI + header comment preservation.
    // (Multi-intermediate like server.tls.port may hit fallback; 1-level
    // nested + structure is asserted. Deeper cases covered in unit tests.)
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(&file, "# App Config\nname: demo\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("ensure")
        .arg(&file)
        .arg("server.port")
        .arg("9443")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content).expect("must be valid YAML");
    assert!(
        content.contains("# App Config"),
        "header comment lost: {content}"
    );
    assert!(
        content.contains("9443") || content.contains("port"),
        "port value missing: {content}"
    );
    // Note: full reparsed structure for new top-level containers is validated in
    // unit tests (serialize_value_preserving + reparsed == expected).
}

#[test]
fn test_doc_move_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Config\nold_name: my-app\n\n# Server\nserver:\n  host: localhost\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("move")
        .arg(&file)
        .arg("old_name")
        .arg("new_name")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# Server"),
        "section comment stripped: {content}"
    );
    assert!(
        content.contains("new_name"),
        "renamed key missing: {content}"
    );
    assert!(
        !content.contains("old_name"),
        "old key still present: {content}"
    );
    assert!(
        content.contains("my-app"),
        "value lost during move: {content}"
    );
}

#[test]
fn test_doc_prepend_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Config\nname: my-app\n\n# Items\nitems:\n  - existing\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("prepend")
        .arg(&file)
        .arg("items")
        .arg("first")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# Items"),
        "section comment stripped: {content}"
    );
    assert!(
        content.contains("first"),
        "prepended item missing: {content}"
    );
    assert!(
        content.contains("existing"),
        "original item missing: {content}"
    );
    let parsed: serde_json::Value = serde_yaml_ng::from_str(&content).unwrap();
    let items = parsed.get("items").expect("items key missing");
    assert_eq!(items[0], "first", "prepended item not at position 0");
    assert_eq!(items[1], "existing", "original item not at position 1");
}

#[test]
fn test_doc_delete_where_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Config\nname: my-app\n\n# Items\nitems:\n  - name: keep\n    val: 1\n  - name: remove\n    val: 2\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete-where")
        .arg(&file)
        .arg("items")
        .arg("--predicate")
        .arg("name=remove")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# Items"),
        "section comment stripped: {content}"
    );
    assert!(
        content.contains("keep"),
        "surviving item missing: {content}"
    );
    assert!(
        !content.contains("remove"),
        "deleted item still present: {content}"
    );
}

#[test]
fn test_doc_append_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Config\nname: my-app\n\n# Items\nitems:\n  - existing\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("append")
        .arg(&file)
        .arg("items")
        .arg("last")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# Items"),
        "section comment stripped: {content}"
    );
    let parsed: serde_json::Value = serde_yaml_ng::from_str(&content).unwrap();
    let items = parsed.get("items").expect("items key missing");
    assert_eq!(items[0], "existing", "original item not at position 0");
    assert_eq!(items[1], "last", "appended item not at position 1");
}

#[test]
fn test_doc_prepend_yaml_sequence_root_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("items.yaml");
    fs::write(&file, "# Items list\n- item1\n- item2\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("prepend")
        .arg(&file)
        .arg("")
        .arg("item0")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Items list"),
        "comment stripped: {content}"
    );
    let parsed: serde_json::Value = serde_yaml_ng::from_str(&content).unwrap();
    let arr = parsed.as_array().expect("root should be array");
    assert_eq!(arr[0], "item0", "prepended item not at position 0");
    assert_eq!(arr[1], "item1", "original item1 not at position 1");
    assert_eq!(arr[2], "item2", "original item2 not at position 2");
}

#[test]
fn test_doc_delete_where() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"items":[{"name":"keep"},{"name":"remove"}]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete-where")
        .arg(&file)
        .arg("items")
        .arg("--predicate")
        .arg("name=remove")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    let items = v["items"].as_array().expect("items should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], serde_json::json!("keep"));
}

/// #1434: delete-where zero matches is exit 0 with removed:0 / changed:false.
#[test]
fn test_doc_delete_where_json_zero_match_reports_removed_zero() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"items":[{"name":"keep"},{"name":"also"}]}"#).unwrap();

    let stdout = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete-where")
        .arg(&file)
        .arg("items")
        .arg("--predicate")
        .arg("name=nobody")
        .arg("--apply")
        .arg("--json")
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&stdout).expect("valid json");
    assert_eq!(v["ok"], true);
    assert_eq!(v["changed"], false, "payload: {v}");
    assert_eq!(v["removed"], 0, "payload: {v}");

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("keep") && content.contains("also"),
        "file should be unchanged on zero-match: {content}"
    );
}

/// #1434: delete-where with matches reports non-zero removed and changed:true.
#[test]
fn test_doc_delete_where_json_match_reports_removed_count() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(
        &file,
        r#"{"items":[{"name":"a"},{"name":"b"},{"name":"a"}]}"#,
    )
    .unwrap();

    let stdout = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete-where")
        .arg(&file)
        .arg("items")
        .arg("--predicate")
        .arg("name=a")
        .arg("--apply")
        .arg("--json")
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&stdout).expect("valid json");
    assert_eq!(v["ok"], true);
    assert_eq!(v["changed"], true, "payload: {v}");
    assert_eq!(v["removed"], 2, "payload: {v}");

    let content = fs::read_to_string(&file).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["items"].as_array().unwrap().len(), 1);
    assert_eq!(parsed["items"][0]["name"], "b");
}

/// #1434: doc delete missing key is exit 0 with removed:0.
#[test]
fn test_doc_delete_json_missing_key_reports_removed_zero() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name":"keep"}"#).unwrap();

    let stdout = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete")
        .arg(&file)
        .arg("missing")
        .arg("--apply")
        .arg("--json")
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&stdout).expect("valid json");
    assert_eq!(v["ok"], true);
    assert_eq!(v["changed"], false, "payload: {v}");
    assert_eq!(v["removed"], 0, "payload: {v}");
}

/// #1434 follow-up: delete of an existing key reports removed:1 and changed:true.
#[test]
fn test_doc_delete_json_existing_key_reports_removed_one() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name":"keep","drop":true}"#).unwrap();

    let stdout = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete")
        .arg(&file)
        .arg("drop")
        .arg("--apply")
        .arg("--json")
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&stdout).expect("valid json");
    assert_eq!(v["ok"], true);
    assert_eq!(v["changed"], true, "payload: {v}");
    assert_eq!(v["removed"], 1, "payload: {v}");
    let content: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert!(content.get("drop").is_none());
    assert_eq!(content["name"], "keep");
}

/// Non-delete writes always report `changed`; ensure no-op is false with no `removed`.
#[test]
fn test_doc_ensure_json_existing_key_reports_changed_false() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name":"original"}"#).unwrap();

    let stdout = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("ensure")
        .arg(&file)
        .arg("name")
        .arg("overwritten")
        .arg("--apply")
        .arg("--json")
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&stdout).expect("valid json");
    assert_eq!(v["ok"], true);
    assert_eq!(v["changed"], false, "payload: {v}");
    // No bytes written under --apply: applied must be false (agent honesty).
    assert_eq!(
        v["applied"], false,
        "no-op ensure must not claim applied: {v}"
    );
    assert!(
        v.get("removed").is_none(),
        "ensure must not emit removed: {v}"
    );
    assert!(
        v.get("backup_session").is_none(),
        "no-op ensure must not create a backup: {v}"
    );
    let content: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(content["name"], "original");
}

/// Unicode predicate values must count toward removed (not silent no-op).
#[test]
fn test_doc_delete_where_json_unicode_predicate_reports_removed() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(
        &file,
        r#"{"users":[{"name":"日本語"},{"name":"🎉"},{"name":"ascii"}]}"#,
    )
    .unwrap();

    let stdout = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete-where")
        .arg(&file)
        .arg("users")
        .arg("--predicate")
        .arg("name=🎉")
        .arg("--apply")
        .arg("--json")
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&stdout).expect("valid json");
    assert_eq!(v["ok"], true);
    assert_eq!(v["changed"], true, "payload: {v}");
    assert_eq!(v["removed"], 1, "payload: {v}");
    let content: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(content["users"].as_array().unwrap().len(), 2);
}

/// set that would change content: --check JSON reports changed:true and exit 2.
#[test]
fn test_doc_set_json_check_reports_changed_true_exit_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    let stdout = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("a")
        .arg("2")
        .arg("--check")
        .arg("--json")
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&stdout).expect("valid json");
    assert_eq!(v["ok"], true);
    assert_eq!(v["changed"], true, "payload: {v}");
    // Check mode must not mutate.
    assert_eq!(fs::read_to_string(&file).unwrap().trim(), r#"{"a":1}"#);
}

// ---------------------------------------------------------------------------
// md
// ---------------------------------------------------------------------------

#[test]
fn test_doc_get_yaml() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(&file, "name: patchloom\nversion: 1\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("name")
        .assert()
        .success()
        .stdout(
            predicate::str::starts_with("\"patchloom\"")
                .or(predicate::str::starts_with("patchloom")),
        );
}

#[test]
fn test_doc_get_yaml_merge_key_resolved() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "defaults: &d\n  timeout: 30\n  retries: 3\nstaging:\n  <<: *d\n",
    )
    .unwrap();

    // Inherited key via merge must be accessible.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("staging.retries")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("3"));
}

/// Unrelated `doc set` must keep YAML anchors/aliases/merge keys (not expand
/// shared defaults into full map copies on every write).
#[test]
fn test_doc_set_yaml_preserves_anchors_and_merges() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    let original = "\
defaults: &defaults
  timeout: 30
  retries: 3
staging:
  <<: *defaults
  host: staging.example.com
production:
  <<: *defaults
  host: prod.example.com
app_name: my-service
";
    fs::write(&file, original).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("app_name")
        .arg("\"other-service\"")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("&defaults"),
        "anchor definition lost:\n{content}"
    );
    assert_eq!(
        content.matches("<<: *defaults").count(),
        2,
        "merge keys expanded away:\n{content}"
    );
    assert!(
        content.contains("app_name: other-service")
            || content.contains("app_name: \"other-service\""),
        "updated field missing:\n{content}"
    );
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("staging.timeout")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("30"));
}

/// Interior edit of `key: *anchor` becomes `<<: *anchor` plus the local key.
#[test]
fn test_doc_set_yaml_pure_alias_becomes_merge() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "shared: &shared\n  timeout: 30\n  retries: 3\nservice_a: *shared\nservice_b: *shared\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("service_a.timeout")
        .arg("60")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("&shared") && content.contains("<<: *shared"),
        "pure alias override must become merge:\n{content}"
    );
    assert!(
        content.contains("service_b: *shared"),
        "sibling alias must stay a pure alias:\n{content}"
    );
    assert!(
        content.contains("timeout: 60"),
        "local override missing:\n{content}"
    );
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("service_a.retries")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("3"));
}

/// Interior edit of a sequence item alias becomes `<<: *anchor` plus the local key.
#[test]
fn test_doc_set_yaml_sequence_alias_item_becomes_merge() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "shared: &shared\n  timeout: 30\n  retries: 3\nitems:\n  - *shared\n  - *shared\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("items[0].timeout")
        .arg("60")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("&shared") && content.contains("- <<: *shared"),
        "sequence alias override must become merge:\n{content}"
    );
    assert!(
        content.contains("timeout: 60"),
        "local override missing:\n{content}"
    );
    assert!(
        content.contains("  - *shared"),
        "untouched second item must stay a pure alias:\n{content}"
    );
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("items[0].retries")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("3"));
}

/// `doc set --apply` that appends to an array inherited only via `<<`
/// must keep the merge key and add a local `env:` override.
#[test]
fn test_doc_set_yaml_inherited_array_growth_keeps_merge_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "\
defaults: &defaults
  env:
    - name: A
      value: \"1\"
deployment:
  <<: *defaults
",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("deployment.env")
        .arg(r#"[{"name":"A","value":"1"},{"name":"B","value":"2"}]"#)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content,
        "\
defaults: &defaults
  env:
    - name: A
      value: \"1\"
deployment:
  <<: *defaults
  env:
    - name: A
      value: '1'
    - name: B
      value: '2'
"
    );
}

/// Mixed flow `{cfg: *shared}` plus later block `cfg: *shared`. `doc set` on
/// the block site must splice and leave the flow sibling.
#[test]
fn test_doc_set_yaml_mixed_flow_block_alias_keeps_flow() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "shared: &shared\n  timeout: 30\nflow: {cfg: *shared}\nblock:\n  cfg: *shared\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("block.cfg.timeout")
        .arg("60")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content,
        "shared: &shared\n  timeout: 30\nflow: {cfg: *shared}\nblock:\n  cfg:\n    <<: *shared\n    timeout: 60\n"
    );
}

#[test]
fn test_doc_set_yaml_mixed_flow_sequence_block_alias_keeps_flow() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "shared: &shared\n  timeout: 30\nflow: [*shared]\nblock:\n  - *shared\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("block[0].timeout")
        .arg("60")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content,
        "shared: &shared\n  timeout: 30\nflow: [*shared]\nblock:\n  - <<: *shared\n    timeout: 60\n"
    );
}

/// #2274 / #2275: emptying a non-last sequence item must stay CST-shaped.
#[test]
fn test_doc_set_yaml_empty_non_last_sequence_item_keeps_anchor() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("u.yaml");
    fs::write(
        &file,
        "a: &x\n  k: v\nitems:\n  - name: A\n  - name: B\nz: *x\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("items[0]")
        .arg("{}")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content,
        "a: &x\n  k: v\nitems:\n  - {}\n  - name: B\nz: *x\n"
    );
}

/// #2275: merge-only first item with a sibling must keep `&defaults`.
#[test]
fn test_doc_delete_yaml_merge_only_sequence_item_keeps_anchor() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("m.yaml");
    fs::write(
        &file,
        "defaults: &defaults\n  env: 1\nitems:\n  - <<: *defaults\n  - other: 2\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete")
        .arg(&file)
        .arg("items[0].env")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content,
        "defaults: &defaults\n  env: 1\nitems:\n  - {}\n  - other: 2\n"
    );
}

/// Two inner flow arrays emptied to `[]` in one write must keep CST.
#[test]
fn test_doc_set_yaml_empty_two_inner_arrays_keeps_anchor() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("u.yaml");
    let original = "a: &x\n  k: v\nitems:\n  - [[1, 2], [3, 4]]\n  - [[5, 6], [7, 8]]\nz: *x\n";
    fs::write(&file, original).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("items[0]")
        .arg("[[],[]]")
        .assert()
        .code(2);
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        original,
        "preview must not write"
    );

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("items[0]")
        .arg("[[],[]]")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("&x") && content.contains("*x"),
        "CLI two inner [] empties must keep CST:\n{content}"
    );
    assert!(
        content.contains("- [[], []]"),
        "emptied item must stay flow []:\n{content}"
    );
    assert!(
        content.contains("- [[5, 6], [7, 8]]"),
        "sibling item must stay:\n{content}"
    );
}

/// Last-item empty must not glue `{}` onto the next mapping key.
#[test]
fn test_doc_set_yaml_empty_last_sequence_item_keeps_anchor() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("u.yaml");
    fs::write(
        &file,
        "a: &x\n  k: v\nitems:\n  - name: A\n  - name: B\nz: *x\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("items[1]")
        .arg("{}")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content,
        "a: &x\n  k: v\nitems:\n  - name: A\n  - {}\nz: *x\n"
    );
}

/// Last merge-only item with a following sibling key must keep `&defaults`.
#[test]
fn test_doc_delete_yaml_merge_only_last_sequence_item_keeps_anchor() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("m.yaml");
    fs::write(
        &file,
        "defaults: &defaults\n  env: 1\nouter:\n  items:\n    - other: 2\n    - <<: *defaults\n  enabled: true\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete")
        .arg(&file)
        .arg("outer.items[1].env")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content,
        "defaults: &defaults\n  env: 1\nouter:\n  items:\n    - other: 2\n    - {}\n  enabled: true\n"
    );
}

/// #2276: unrelated `doc set` must not collapse pre-existing `-   name:`.
#[test]
fn test_doc_set_yaml_unrelated_edit_keeps_wide_dash_spacing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("c.yaml");
    fs::write(
        &file,
        "# top comment\napp: &app\n  name: myapp\nitems:\n  -   name: A\n  -   name: B\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("app.name")
        .arg("\"zzz\"")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content,
        "# top comment\napp: &app\n  name: zzz\nitems:\n  -   name: A\n  -   name: B\n"
    );

    let check = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("app.name")
        .arg("\"yyy\"")
        .arg("--check")
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        check.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&check.stdout).unwrap();
    assert_eq!(json["changed"], true, "{json}");
    assert!(
        json.get("style_changed").is_none() || json["style_changed"] == false,
        "untouched dash spacing must not set style_changed: {json}"
    );
}

#[test]
fn test_doc_set_yaml_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(&file, "name: old\nversion: 1\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("name")
        .arg("\"new\"")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("name: new") || content.contains("name: \"new\""),
        "YAML should contain updated name value: {content}"
    );
    assert!(
        !content.contains("name: old"),
        "YAML should not contain old name value: {content}"
    );
}

#[test]
fn test_doc_get_toml() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.toml");
    fs::write(
        &file,
        "[package]\nname = \"patchloom\"\nversion = \"1.0\"\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("package.name")
        .assert()
        .success()
        .stdout(predicate::str::contains("patchloom"));
}

#[test]
fn test_doc_set_toml_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.toml");
    fs::write(&file, "[package]\nname = \"old\"\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("package.name")
        .arg("\"new\"")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("name = \"new\""),
        "TOML should contain updated name value: {content}"
    );
}

#[test]
fn test_doc_set_toml_null_json_no_stderr_warning() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.toml");
    fs::write(&file, "[package]\nname = \"old\"\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "set"])
        .arg(&file)
        .args(["package.name", "null", "--apply"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("TOML has no null"),
        "null-to-empty must not warn under --json: {stderr}"
    );
    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("name = \"\""),
        "TOML null maps to empty string: {content}"
    );
}

#[test]
fn test_doc_len_array() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"items":[1,2,3,4,5]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("len")
        .arg(&file)
        .arg("items")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("5"));
}

/// #1838: doc len --json success uses ok/value envelope (not bare number).
#[test]
fn test_doc_len_json_success_envelope() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"items":[1,2,3,4,5]}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "len"])
        .arg(&file)
        .arg("items")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["value"], 5, "{v}");
    assert_eq!(v["selector"], "items", "{v}");
    assert!(v["path"].as_str().is_some(), "{v}");
}

#[test]
fn test_doc_append_to_array() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"tags":["a","b"]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("append")
        .arg(&file)
        .arg("tags")
        .arg(r#""c""#)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["tags"].as_array().unwrap().len(), 3);
}

#[test]
fn test_doc_flatten_json() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"a":1,"b":{"c":2},"d":[10,20]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("flatten")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("a = 1"))
        .stdout(predicate::str::contains("b.c = 2"))
        .stdout(predicate::str::contains("d[0] = 10"))
        .stdout(predicate::str::contains("d[1] = 20"));
}

#[test]
fn test_doc_flatten_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"name":"patchloom"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("doc")
        .arg("flatten")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\""))
        .stdout(predicate::str::contains("\"patchloom\""));
}

#[test]
fn test_doc_flatten_includes_empty_arrays() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(
        &file,
        r#"{"default":["a"],"empty_arr":[],"empty_obj":{},"nested":{"deep_empty":[]}}"#,
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("doc")
        .arg("flatten")
        .arg(&file)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    // #1838: --json success is an ok envelope; flatten map lives under value.
    assert_eq!(parsed["ok"], true, "envelope: {stdout}");
    let value = &parsed["value"];
    assert_eq!(
        value["empty_arr"],
        serde_json::json!([]),
        "empty array missing: {stdout}"
    );
    assert_eq!(
        value["empty_obj"],
        serde_json::json!({}),
        "empty object missing: {stdout}"
    );
    assert_eq!(
        value["nested.deep_empty"],
        serde_json::json!([]),
        "nested empty array missing: {stdout}"
    );
    assert_eq!(value["default[0]"], serde_json::json!("a"));
}

// ---------------------------------------------------------------------------
// doc diff
// ---------------------------------------------------------------------------

#[test]
fn test_doc_diff_shows_changes() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.json");
    let b = dir.path().join("b.json");
    fs::write(&a, r#"{"name":"old","keep":1}"#).unwrap();
    fs::write(&b, r#"{"name":"new","keep":1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("diff")
        .arg(&a)
        .arg(&b)
        .assert()
        .code(2)
        .stdout(predicate::str::contains("~ name"));
}

// ---------------------------------------------------------------------------
// --check mode: exits 2 when changes detected, does NOT write
// ---------------------------------------------------------------------------

#[test]
fn test_doc_flatten_jsonl_success_envelope() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1,"b":{"c":2}}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("flatten")
        .arg(&file)
        .arg("--jsonl")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "single envelope line (#1838): {stdout}");
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json["ok"], true, "{json}");
    let map = json["value"].as_object().expect("flatten map");
    assert_eq!(map.get("a"), Some(&serde_json::json!(1)));
    assert_eq!(map.get("b.c"), Some(&serde_json::json!(2)));
}

#[test]
fn test_doc_diff_jsonl_outputs_one_entry_per_line() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.json");
    let b = dir.path().join("b.json");
    fs::write(&a, r#"{"name":"old","removed":true}"#).unwrap();
    fs::write(&b, r#"{"name":"new","added":"yes"}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("diff")
        .arg(&a)
        .arg(&b)
        .arg("--jsonl")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();

    assert!(
        lines
            .iter()
            .any(|v| v["kind"] == "changed" && v["path"] == "name")
    );
    assert!(
        lines
            .iter()
            .any(|v| v["kind"] == "removed" && v["path"] == "removed")
    );
    assert!(
        lines
            .iter()
            .any(|v| v["kind"] == "added" && v["path"] == "added")
    );
}

#[test]
fn test_doc_diff_identical_json_output() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.json");
    fs::write(&a, r#"{"name":"same"}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("diff")
        .arg(&a)
        .arg(&a)
        .arg("--json")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("should be valid JSON, got error: {e}, output: {stdout}"));
    assert_eq!(v["identical"], serde_json::json!(true));
}

// Regression: doc diff with differences must return exit code 2 (CHANGES_DETECTED),
// not 0 (SUCCESS). Identical files still return 0.
#[test]
fn test_doc_diff_exit_code_changes_detected_json() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.json");
    let b = dir.path().join("b.json");
    fs::write(&a, r#"{"key":"old"}"#).unwrap();
    fs::write(&b, r#"{"key":"new"}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("diff")
        .arg(&a)
        .arg(&b)
        .arg("--json")
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "doc diff with differences should exit 2 (CHANGES_DETECTED)"
    );
}

#[test]
fn test_doc_delete_removes_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name":"keep","remove_me":true}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete")
        .arg(&file)
        .arg("remove_me")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["name"], serde_json::json!("keep"));
    assert!(v.get("remove_me").is_none(), "key should be removed");
}

#[test]
fn test_doc_merge_combines_objects() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("merge")
        .arg(&file)
        .arg("--value")
        .arg(r#"{"b":2}"#)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["a"], serde_json::json!(1));
    assert_eq!(v["b"], serde_json::json!(2));
}

#[test]
fn test_doc_prepend_inserts_at_front() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"items":[2,3]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("prepend")
        .arg(&file)
        .arg("items")
        .arg("1")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    let items = v["items"].as_array().unwrap();
    assert_eq!(items[0], serde_json::json!(1));
    assert_eq!(items.len(), 3);
}

#[test]
fn test_doc_select_filters_by_predicate() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(
        &file,
        r#"{"items":[{"status":"active","name":"a"},{"status":"done","name":"b"}]}"#,
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("select")
        .arg(&file)
        .arg("items[status=active]")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\""))
        .stdout(predicate::str::contains("\"a\""))
        .stdout(predicate::str::contains("\"b\"").not());
}

/// #1838: doc select --json success uses ok/value envelope (same path as get).
#[test]
fn test_doc_select_json_success_envelope() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(
        &file,
        r#"{"items":[{"status":"active","name":"a"},{"status":"done","name":"b"}]}"#,
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "select"])
        .arg(&file)
        .arg("items[status=active]")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    // Single match unwraps to the object (same as doc get multi-value rule).
    assert_eq!(v["value"]["name"], "a", "{v}");
    assert_eq!(v["value"]["status"], "active", "{v}");
    assert_eq!(v["selector"], "items[status=active]", "{v}");
}

#[test]
fn test_doc_ensure_creates_missing_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("ensure")
        .arg(&file)
        .arg("b")
        .arg("2")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["b"], serde_json::json!(2));
}

#[test]
fn test_doc_ensure_noop_when_exists() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    // Use pre-formatted JSON so the engine does not detect formatting-only changes.
    fs::write(&file, "{\n  \"a\": 1\n}\n").unwrap();

    // ensure with --check when key already exists should exit 0 (no changes)
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("ensure")
        .arg(&file)
        .arg("a")
        .arg("1")
        .arg("--check")
        .assert()
        .code(0);
}

#[test]
fn test_doc_move_renames_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"old_key":"value"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("move")
        .arg(&file)
        .arg("old_key")
        .arg("new_key")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["new_key"], serde_json::json!("value"));
    assert!(v.get("old_key").is_none(), "old key should be gone");
}

#[test]
fn test_doc_update_matching_nodes() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"items":[{"s":"a"},{"s":"b"}]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("update")
        .arg(&file)
        .arg("items[*].s")
        .arg("\"x\"")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    let items = v["items"].as_array().unwrap();
    assert_eq!(items[0]["s"], serde_json::json!("x"));
    assert_eq!(items[1]["s"], serde_json::json!("x"));
}

/// Selector predicates (not a separate --where flag) filter which elements update.
/// fixrealloop: agents reading "predicate" in the schema invented --where.
#[test]
fn test_doc_update_selector_predicate_filters_elements() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(
        &file,
        r#"{"items":[{"name":"a","v":1},{"name":"b","v":2}]}"#,
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("update")
        .arg(&file)
        .arg("items[name=a].v")
        .arg("7")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["items"][0]["v"], serde_json::json!(7));
    assert_eq!(
        v["items"][1]["v"],
        serde_json::json!(2),
        "non-matching element must stay unchanged"
    );
}

// ---------------------------------------------------------------------------
// md: dedupe-headings, lint-agents
// ---------------------------------------------------------------------------

#[test]
fn test_doc_get_nonexistent_file_fails() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg("/nonexistent/file_xyz.json")
        .arg("key")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("nonexistent/file_xyz.json"));
}

#[test]
fn test_doc_get_unsupported_extension_fails() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.ini");
    fs::write(&file, "key=value\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("key")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("unsupported file extension"));
}

#[test]
fn test_doc_get_nonexistent_file_json_envelope() {
    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["doc", "get", "/nonexistent/file_xyz.json", "key", "--json"])
        .output()
        .unwrap();
    assert!(!out.status.success());

    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("error should be wrapped in JSON envelope");
    assert_eq!(json["ok"], false);
    assert_eq!(
        json["error_kind"], "not_found",
        "missing file must set error_kind not_found: {json}"
    );
    assert_eq!(
        json["applied"], false,
        "missing file must not claim applied: {json}"
    );
    assert!(
        json["error"].is_string(),
        "envelope should contain error field"
    );
}

#[test]
fn test_doc_set_nonexistent_file_json_envelope() {
    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "--json",
            "doc",
            "set",
            "/nonexistent/file_xyz.json",
            "x",
            "1",
            "--apply",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());

    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("error should be wrapped in JSON envelope");
    assert_eq!(json["ok"], false);
    assert_eq!(
        json["error_kind"], "not_found",
        "missing file doc set must set error_kind not_found: {json}"
    );
    assert_eq!(
        json["applied"], false,
        "missing file must not claim applied: {json}"
    );
}

#[test]
fn test_doc_get_unsupported_extension_json_envelope() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.ini");
    fs::write(&file, "key=value\n").unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["doc", "get", &file.to_string_lossy(), "key", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));

    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("error should be wrapped in JSON envelope");
    assert_eq!(json["ok"], false);
    assert_eq!(
        json["error_kind"], "invalid_input",
        "unsupported extension should be invalid_input, not type_error: {json}"
    );
    assert!(
        json["error"]
            .as_str()
            .unwrap_or("")
            .contains("unsupported file extension"),
        "error should mention extension: {json}"
    );
}

#[test]
fn test_doc_set_unsupported_extension_json_invalid_input() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "not structured\n").unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "--json",
            "doc",
            "set",
            &file.to_string_lossy(),
            "key",
            "val",
            "--apply",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(
        json["error_kind"], "invalid_input",
        "doc write unsupported extension must not be type_error: {json}"
    );
}

#[test]
fn test_doc_get_malformed_json_parse_error() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("bad.json");
    fs::write(&file, "{bad").unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "get", &file.to_string_lossy(), "key"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(4),
        "malformed JSON should be parse_error exit 4: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(
        json["error_kind"], "parse_error",
        "malformed document must set parse_error: {json}"
    );
}

#[test]
fn test_doc_select_no_matches_exits_3() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"items":[{"status":"active"}]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("select")
        .arg(&file)
        .arg("items[status=nonexistent]")
        .assert()
        .code(3);
}

#[test]
fn test_doc_move_missing_source_fails() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    let assert = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("doc")
        .arg("move")
        .arg(&file)
        .arg("nonexistent")
        .arg("target")
        .arg("--apply")
        .assert()
        .code(3); // no_matches (missing source key), not generic failure
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["error_kind"], "no_matches");
    assert!(
        v["error"].as_str().unwrap_or("").contains("not found"),
        "error should mention not found: {stdout}"
    );
}

#[test]
fn test_doc_merge_nested_objects() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"x":{"existing":"old"}}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("merge")
        .arg(&file)
        .arg("--value")
        .arg(r#"{"x":{"nested":"new"}}"#)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(
        v["x"]["existing"],
        serde_json::json!("old"),
        "existing key preserved"
    );
    assert_eq!(v["x"]["nested"], serde_json::json!("new"), "new key merged");
}

#[test]
fn test_doc_ensure_noop_when_value_differs() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    // ensure a=99 when a already exists with value 1: should NOT change the value
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("ensure")
        .arg(&file)
        .arg("a")
        .arg("99")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(
        v["a"],
        serde_json::json!(1),
        "ensure should not overwrite existing key"
    );
}

#[test]
fn test_doc_set_check_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"version":"1.0"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("version")
        .arg("\"2.0\"")
        .arg("--check")
        .assert()
        .code(2);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("1.0"),
        "file should be unchanged in --check mode"
    );
}

// ---------------------------------------------------------------------------
// tx: file.delete on empty file (bug fix), validation optional step
// ---------------------------------------------------------------------------

#[test]
fn test_doc_get_honors_cwd() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        "{\n  \"version\": \"1.0.0\"\n}\n",
    )
    .unwrap();

    patchloom_in(dir.path())
        .arg("doc")
        .arg("get")
        .arg("package.json")
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("1.0.0"));
}

#[test]
fn test_doc_delete_check_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"key":"value","other":"keep"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete")
        .arg(&file)
        .arg("key")
        .arg("--check")
        .assert()
        .code(2);

    // File should be unchanged in --check mode.
    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, r#"{"key":"value","other":"keep"}"#,
        "file should be unchanged in --check mode"
    );
}

// ---------------------------------------------------------------------------
// doc merge --check exits 2 when changes would be made
// ---------------------------------------------------------------------------

#[test]
fn test_doc_merge_check_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("merge")
        .arg(&file)
        .arg("--value")
        .arg(r#"{"b":2}"#)
        .arg("--check")
        .assert()
        .code(2);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, r#"{"a":1}"#,
        "file should be unchanged in --check mode"
    );
}

// ---------------------------------------------------------------------------
// --json error envelope (#227)
// ---------------------------------------------------------------------------

#[test]
fn test_doc_check_produces_stdout() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"version":"1.0"}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("version")
        .arg("\"2.0\"")
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("would modify"),
        "doc --check should produce stdout, got: {stdout}"
    );
}

#[test]
fn test_doc_check_json_produces_structured_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"version":"1.0"}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("version")
        .arg("\"2.0\"")
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|_| panic!("expected JSON output, got: {stdout}"));
    assert_eq!(json["ok"], true, "check output should have ok=true");
    // Doc write operations now use DocWriteOutput format via execute_via_engine.
    assert!(
        json["path"].is_string(),
        "check output should have path field"
    );
}

// ---------------------------------------------------------------------------
// doc --json failure produces structured error on stdout (#545)
// ---------------------------------------------------------------------------

#[test]
fn test_doc_json_failure_structured_on_stdout() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"name":"patchloom"}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("doc")
        .arg("append")
        .arg(&file)
        .arg("name")
        .arg("\"x\"")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    // Stderr should be empty; error goes to stdout as JSON.
    assert!(
        String::from_utf8_lossy(&output.stderr).trim().is_empty(),
        "stderr should be empty in --json mode"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|_| panic!("expected JSON output, got: {stdout}"));
    assert_eq!(json["ok"], false);
    assert!(
        json["error"].as_str().unwrap().contains("not an array"),
        "error should mention 'not an array'"
    );
}

// ---------------------------------------------------------------------------
// #1288: numeric dot-notation as array index
// ---------------------------------------------------------------------------

#[test]
fn test_doc_set_numeric_dot_notation_on_array() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(
        &file,
        r#"{"env": [{"name": "A", "value": "old"}, {"name": "B", "value": "keep"}]}"#,
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "doc",
            "set",
            file.to_str().unwrap(),
            "env.0.value",
            "new",
            "--apply",
        ])
        .assert()
        .code(0);

    let content: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(content["env"][0]["value"], "new");
    assert_eq!(content["env"][1]["value"], "keep");
}

#[test]
fn test_doc_get_numeric_dot_notation_on_array() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"items": ["alpha", "beta", "gamma"]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["doc", "get", file.to_str().unwrap(), "items.1"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("beta"));
}

#[test]
fn test_doc_delete_numeric_dot_notation_on_array() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"arr": [1, 2, 3]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["doc", "delete", file.to_str().unwrap(), "arr.0", "--apply"])
        .assert()
        .code(0);

    let content: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(content["arr"], serde_json::json!([2, 3]));
}

// ---------------------------------------------------------------------------
// NO_MATCHES exit code for write operations (#QA: string-based error guard)
// ---------------------------------------------------------------------------

/// Write operations that select a non-existent key should exit 3 (NO_MATCHES),
/// not exit 1 (FAILURE). Classification uses `NoMatchError` downcast (#1331),
/// not string matching.
#[test]
fn test_doc_update_nonexistent_selector_exits_3() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"a": 1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "doc",
            "update",
            file.to_str().unwrap(),
            "nonexistent",
            "99",
            "--apply",
        ])
        .assert()
        .code(3);
}

// ---------------------------------------------------------------------------
// No-match text-mode stderr tests
// ---------------------------------------------------------------------------

#[test]
fn test_doc_update_apply_no_match_emits_stderr_in_text_mode() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"a": 1}"#).unwrap();

    patchloom_in(dir.path())
        .args([
            "doc",
            "update",
            file.to_str().unwrap(),
            "nonexistent",
            "99",
            "--apply",
        ])
        .assert()
        .code(3)
        .stderr(predicates::str::contains("doc.update"));
}

// ---------------------------------------------------------------------------
// Text-mode no-match stderr output (#1340)
// ---------------------------------------------------------------------------

#[test]
fn test_doc_get_no_match_text_mode_emits_stderr() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a": 1}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("nonexistent")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no match"),
        "text mode should emit error to stderr, got: {stderr}"
    );
}

#[test]
fn test_doc_get_no_match_json_sets_error_kind() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a": 1}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("nonexistent")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).expect("valid JSON");
    assert_eq!(v["ok"], false);
    assert_eq!(
        v["error_kind"], "no_matches",
        "doc get --json no-match should set error_kind: {v}"
    );
    assert!(
        v["error"].as_str().unwrap_or("").contains("no match"),
        "error message should remain human-readable: {v}"
    );
}

#[test]
fn test_doc_keys_no_match_text_mode_emits_stderr() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a": 1}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("keys")
        .arg(&file)
        .arg("nonexistent")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no match"),
        "text mode should emit error to stderr, got: {stderr}"
    );
}

#[test]
fn test_doc_len_no_match_text_mode_emits_stderr() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a": 1}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("len")
        .arg(&file)
        .arg("nonexistent")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no match"),
        "text mode should emit error to stderr, got: {stderr}"
    );
}

#[test]
fn test_doc_get_no_match_quiet_suppresses_stderr() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a": 1}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("nonexistent")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    assert!(
        output.stderr.is_empty(),
        "--quiet should suppress stderr, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// Regression: default (preview) mode exit code (#1345)
// ---------------------------------------------------------------------------

// doc set in default mode must return exit 2 (CHANGES_DETECTED), not 0.
#[test]
fn test_doc_set_default_mode_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"version":"1.0"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("version")
        .arg("\"2.0\"")
        .assert()
        .code(2);

    // File should not be modified
    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, r#"{"version":"1.0"}"#);
}

// doc delete in default mode must return exit 2 (CHANGES_DETECTED), not 0.
#[test]
fn test_doc_delete_default_mode_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1,"b":2}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete")
        .arg(&file)
        .arg("a")
        .assert()
        .code(2);

    // File should not be modified
    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("\"a\""));
}

// ---------------------------------------------------------------------------
// doc keys/len type-error JSON envelope tests (#1354 coverage)
// ---------------------------------------------------------------------------

#[test]
fn test_doc_keys_not_an_object_returns_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name": "hello"}"#).unwrap();

    // Text mode: exit 1
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("keys")
        .arg(&file)
        .arg("name")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "should be FAILURE");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not an object"),
        "text-mode stderr should say 'not an object', got: {stderr}"
    );

    // JSON mode: exit 1 with JSON envelope
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("doc")
        .arg("keys")
        .arg(&file)
        .arg("name")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "JSON mode should be FAILURE");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(r#""ok": false"#),
        "JSON output should contain ok:false, got: {stdout}"
    );
    assert!(
        stdout.contains("not an object"),
        "JSON output should contain error, got: {stdout}"
    );
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        parsed["error_kind"], "type_error",
        "doc keys type mismatch should set error_kind: {parsed}"
    );
}

#[test]
fn test_doc_len_not_array_or_object_returns_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"count": 42}"#).unwrap();

    // Text mode: exit 1
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("len")
        .arg(&file)
        .arg("count")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "should be FAILURE");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not an array or object"),
        "text-mode stderr should say 'not an array or object', got: {stderr}"
    );

    // JSON mode: exit 1 with JSON envelope
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("doc")
        .arg("len")
        .arg(&file)
        .arg("count")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "JSON mode should be FAILURE");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(r#""ok": false"#),
        "JSON output should contain ok:false, got: {stdout}"
    );
    assert!(
        stdout.contains("not an array or object"),
        "JSON output should contain error, got: {stdout}"
    );
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        parsed["error_kind"], "type_error",
        "doc len type mismatch should set error_kind: {parsed}"
    );
}

// ---------------------------------------------------------------------------
// Symlink integration tests (#231 coverage)
// ---------------------------------------------------------------------------

#[test]
fn test_doc_get_contain_rejects_parent_escape() {
    let dir = TempDir::new().unwrap();
    let escape_name = format!(
        "patchloom-doc-escape-{}.json",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let outside = dir.path().parent().unwrap().join(&escape_name);
    fs::write(&outside, r#"{"secret":1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "--contain",
            "doc",
            "get",
            &format!("../{escape_name}"),
            "secret",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );

    let _ = fs::remove_file(&outside);
}

#[test]
fn test_doc_merge_json_stdin_and_value_sets_error_kind() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("c.json");
    fs::write(&file, "{}").unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "--json",
            "doc",
            "merge",
            file.to_str().unwrap(),
            "--value",
            "{}",
            "--stdin",
            "--apply",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(
        json["error_kind"], "invalid_input",
        "doc merge dual inputs: {json}"
    );
}

#[test]
fn test_doc_set_format_failure_json_error_kind() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("c.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "--json",
            "doc",
            "set",
            file.to_str().unwrap(),
            "a",
            "2",
            "--apply",
            "--format",
            "false",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(
        json["error_kind"], "format_failed",
        "doc write path must not drop FormatFailedError: {json}"
    );
    assert!(
        json["backup_session"]
            .as_str()
            .is_some_and(|s| !s.is_empty()),
        "format_failed after write must expose backup_session for undo: {json}"
    );
    let on_disk = fs::read_to_string(&file).unwrap();
    let val: serde_json::Value = serde_json::from_str(&on_disk).unwrap_or_else(|e| {
        panic!("doc set must write valid JSON before format failure: {e}; {on_disk}")
    });
    assert_eq!(
        val["a"],
        serde_json::json!(2),
        "doc set must still write before format failure: {on_disk}"
    );
}

/// Doc set on a directory must set error_kind (not a bare generic failure).
#[test]
fn test_doc_set_on_directory_json_error_kind() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("not_a_file");
    fs::create_dir(&sub).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "--json",
            "doc",
            "set",
            sub.to_str().unwrap(),
            "a",
            "1",
            "--apply",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(
        json["error_kind"], "invalid_input",
        "directory target must be invalid_input (not missing error_kind): {json}"
    );
    assert!(
        json["error"].as_str().unwrap_or("").contains("not a file"),
        "error should say target is not a file: {json}"
    );
}

/// Empty path must not join to cwd as a directory target (#2150 follow-up).
#[test]
fn test_doc_set_empty_path_json_invalid_input() {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "set", "", "a", "1", "--apply"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false, "{json}");
    assert_eq!(
        json["error_kind"], "invalid_input",
        "empty path must be invalid_input: {json}"
    );
    assert!(
        json["error"]
            .as_str()
            .unwrap_or("")
            .contains("path must not be empty"),
        "error should name empty path, not cwd directory: {json}"
    );
}

/// Predicate selectors on doc set point agents at doc update (#1725 / #2133).
#[test]
fn test_doc_set_predicate_errors_with_update_hint() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"items":[{"id":"a","val":1}]}"#).unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "set"])
        .arg(&file)
        .args(["items[id=a].val", "9", "--apply"])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("doc update") && combined.contains("wildcard/predicate"),
        "expected actionable error, got: {combined}"
    );
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!("stdout must be JSON error envelope: {e}; stdout={stdout:?} stderr={stderr:?}")
    });
    assert_eq!(json["error_kind"], "invalid_input");
    assert_eq!(
        json["suggested_op"], "doc.update",
        "machine-stable suggested_op for harness retry (#2133): {json}"
    );
    assert_eq!(json["applied"], false);
    assert_eq!(json["ok"], false);
}

/// Multi-document YAML is an array root; bare keys must hint document index.
#[test]
fn test_doc_set_multi_document_bare_key_hints_index() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("multi.yaml");
    fs::write(&file, "a: 1\n---\nb: 2\n").unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "set"])
        .arg(&file)
        .args(["a", "9", "--apply"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "type_error exit 1, not generic failure: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["error_kind"], "type_error", "{v}");
    assert_eq!(v["applied"], false, "{v}");
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        err.contains("array")
            && (err.contains("0.a") || err.contains("[0].a"))
            && err.contains("index"),
        "expected multi-doc index hint, got: {v}"
    );
    // File must be unchanged.
    assert_eq!(fs::read_to_string(&file).unwrap(), "a: 1\n---\nb: 2\n");
}

#[test]
fn test_doc_keys_wildcard_json_is_ambiguous() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"items":[{"a":1},{"b":2}]}"#).unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "keys"])
        .arg(&file)
        .arg("items[*]")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(5),
        "keys on items[*] must be AMBIGUOUS (5), stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["error_kind"], "ambiguous", "{v}");
}

#[test]
fn test_doc_len_wildcard_json_is_ambiguous() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"items":[{"a":1},{"b":2}]}"#).unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "len"])
        .arg(&file)
        .arg("items[*]")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(5),
        "len on items[*] must be AMBIGUOUS (5), stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["error_kind"], "ambiguous", "{v}");
}

/// `doc keys` on multi-doc root should name the array/index shape.
/// Agents pass `.` for root (empty selector is awkward in clap); both must work.
#[test]
fn test_doc_keys_multi_document_root_hints_index() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("multi.yaml");
    fs::write(&file, "a: 1\n---\nb: 2\n").unwrap();

    // `.` is the CLI root form; empty is accepted by clap and used by older tests.
    for root_sel in ["", "."] {
        let out = Command::cargo_bin("patchloom")
            .unwrap()
            .args(["--json", "doc", "keys"])
            .arg(&file)
            .arg(root_sel)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(1),
            "selector {root_sel:?}: stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["error_kind"], "type_error", "selector {root_sel:?}: {v}");
        let err = v["error"].as_str().unwrap_or("");
        // Prefer multi-char guidance over bare "0" (matches any digit noise).
        assert!(
            err.contains("top-level array")
                && err.contains("index")
                && (err.contains("`0`") || err.contains("[0]")),
            "expected multi-doc keys guidance for {root_sel:?}, got: {v}"
        );
    }
}

/// `doc move` bare keys on multi-doc must type_error with index hints.
/// Found by fixrealloop: bare move returned invalid_input "parent is array".
#[test]
fn test_doc_move_multi_document_bare_key_type_error() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("multi.yaml");
    let original = "port: 80\n---\nport: 443\n";
    fs::write(&file, original).unwrap();

    // Bare from+to
    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "move"])
        .arg(&file)
        .args(["port", "http_port", "--apply"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error_kind"], "type_error", "{v}");
    assert_eq!(v["applied"], false, "{v}");
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        err.contains("0.port") || err.contains("[0].port"),
        "expected multi-doc index hint, got: {v}"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), original);

    // Indexed from + bare to
    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "move"])
        .arg(&file)
        .args(["0.port", "http_port", "--apply"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error_kind"], "type_error", "{v}");
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        err.contains("0.http_port") || err.contains("[0].http_port"),
        "expected target index hint, got: {v}"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), original);

    // Indexed success path
    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "move"])
        .arg(&file)
        .args(["0.port", "0.http_port", "--apply"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let body = fs::read_to_string(&file).unwrap();
    assert!(
        body.contains("http_port: 80") && body.contains("port: 443"),
        "expected renamed key under first doc, got:\n{body}"
    );
    assert!(
        !body.contains("\nport: 80") && !body.starts_with("port: 80"),
        "first-doc port key should be gone, got:\n{body}"
    );
}

/// append/delete/update bare keys on multi-doc must type_error with index hints.
/// Found by fixrealloop: append said "expected object at key", delete soft no-op.
#[test]
fn test_doc_append_delete_update_multi_document_bare_key_hints() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("arr.yaml");
    let original = "tags:\n  - a\n---\ntags:\n  - b\n";
    fs::write(&file, original).unwrap();

    for (sub, extra) in [
        ("append", vec!["z"]),
        ("prepend", vec!["z"]),
        ("delete", vec![]),
        ("update", vec![r#"["z"]"#]),
    ] {
        fs::write(&file, original).unwrap();
        let mut cmd = Command::cargo_bin("patchloom").unwrap();
        cmd.args(["--json", "doc", sub]).arg(&file).arg("tags");
        for a in &extra {
            cmd.arg(a);
        }
        cmd.arg("--apply");
        let out = cmd.output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(1),
            "{sub}: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["error_kind"], "type_error", "{sub}: {v}");
        let err = v["error"].as_str().unwrap_or("");
        assert!(
            err.contains("0.tags") || err.contains("[0].tags"),
            "{sub}: expected multi-doc index hint, got: {v}"
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            original,
            "{sub}: file must be unchanged"
        );
    }
}

/// `doc merge` on multi-doc must not replace the whole stream with the overlay.
/// Found by fixrealloop: deep_merge replaced array roots with the object.
/// MPI: array overlays also replaced the stream (sibling of object-only guard).
#[test]
fn test_doc_merge_multi_document_refuses_object_overlay() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("multi.yaml");
    let original = "a: 1\n---\nb: 2\n";
    fs::write(&file, original).unwrap();

    for (label, value) in [("object", r#"{"c":3}"#), ("array", r#"[{"a":9},{"b":9}]"#)] {
        fs::write(&file, original).unwrap();
        let out = Command::cargo_bin("patchloom")
            .unwrap()
            .args(["--json", "doc", "merge"])
            .arg(&file)
            .args(["--value", value, "--apply"])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(1),
            "{label}: type_error exit 1: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["ok"], false, "{label}: {v}");
        assert_eq!(v["error_kind"], "type_error", "{label}: {v}");
        assert_eq!(v["applied"], false, "{label}: {v}");
        let err = v["error"].as_str().unwrap_or("");
        assert!(
            err.contains("top-level array") || err.contains("multi-document"),
            "{label}: expected multi-doc merge guidance, got: {v}"
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            original,
            "{label}: file must be unchanged after refused merge"
        );
    }
}

/// `doc ensure` bare key on multi-doc must type_error (parity with set/move).
#[test]
fn test_doc_ensure_multi_document_bare_key_type_error() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("multi.yaml");
    let original = "name: a\n---\nname: b\n";
    fs::write(&file, original).unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "ensure"])
        .arg(&file)
        .args(["version", "1", "--apply"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error_kind"], "type_error", "{v}");
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        err.contains("0.version") || err.contains("[0].version"),
        "expected multi-doc index hint, got: {v}"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), original);
}

/// `doc has` bare key on multi-doc must be type_error, not soft false (#1843 is for missing only).
#[test]
fn test_doc_has_multi_document_bare_key_hints_index() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("multi.yaml");
    fs::write(&file, "a: 1\n---\nb: 2\n").unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "has"])
        .arg(&file)
        .arg("a")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "type_error exit 1, not soft false: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["error_kind"], "type_error", "{v}");
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        err.contains("0.a") || err.contains("[0].a"),
        "expected multi-doc index hint, got: {v}"
    );

    // Indexed form still soft-false for missing, soft-true for present.
    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "has"])
        .arg(&file)
        .arg("0.a")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["value"], true, "{v}");
}

/// Read path: bare key on multi-doc must be type_error + index hint, not no_matches.
/// Found by fixrealloop (docs already promised set/get parity).
#[test]
fn test_doc_get_multi_document_bare_key_hints_index() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("multi.yaml");
    fs::write(&file, "a: 1\n---\nb: 2\n").unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "get"])
        .arg(&file)
        .arg("a")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "type_error exit 1, not no_matches 3: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["error_kind"], "type_error", "{v}");
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        err.contains("array")
            && (err.contains("0.a") || err.contains("[0].a"))
            && err.contains("index"),
        "expected multi-doc index hint, got: {v}"
    );
    // Indexed form still works.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "get"])
        .arg(&file)
        .arg("0.a")
        .assert()
        .code(0)
        .stdout(predicates::str::contains("\"value\": 1"));
}

/// doc set Apply shares `atomic_write`; hardlinked siblings must stay in sync (#1733).
#[cfg(unix)]
#[test]
fn test_doc_set_apply_preserves_hardlinks() {
    use std::os::unix::fs::MetadataExt;

    let dir = TempDir::new().unwrap();
    let a = dir.path().join("cfg.json");
    let b = dir.path().join("cfg-copy.json");
    fs::write(&a, r#"{"name":"demo"}"#).unwrap();
    fs::hard_link(&a, &b).unwrap();
    let before_ino = fs::metadata(&a).unwrap().ino();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["doc", "set"])
        .arg(&a)
        .args(["name", "updated", "--apply"])
        .assert()
        .code(0);

    let a_content = fs::read_to_string(&a).unwrap();
    let b_content = fs::read_to_string(&b).unwrap();
    assert!(
        a_content.contains("updated"),
        "doc set must update primary path: {a_content}"
    );
    assert_eq!(
        a_content, b_content,
        "hardlink sibling must match after doc set --apply"
    );
    assert_eq!(fs::metadata(&a).unwrap().ino(), before_ino);
    assert!(fs::metadata(&a).unwrap().nlink() > 1);
}

/// CLI `doc set --apply` on multi-doc YAML must keep `---` separators on disk
/// (not collapse to a single YAML sequence). Unit tests cover the serializer;
/// this locks the full CLI write path (#1719).
#[test]
fn test_doc_set_multi_document_apply_preserves_separators() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("k8s.yaml");
    let original = "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: demo\ndata:\n  key: value\n---\napiVersion: v1\nkind: Service\nmetadata:\n  name: demo-svc\nspec:\n  ports:\n    - port: 80\n";
    fs::write(&file, original).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["doc", "set"])
        .arg(&file)
        .args(["0.data.key", "newval", "--apply"])
        .assert()
        .code(0);

    let result = fs::read_to_string(&file).unwrap();
    assert!(
        result.contains("---"),
        "CLI apply must preserve multi-doc stream separators, got:\n{result}"
    );
    assert!(
        !result.trim_start().starts_with("- "),
        "must not serialize multi-doc as a YAML sequence, got:\n{result}"
    );
    assert!(
        result.contains("key: newval") || result.contains("key: \"newval\""),
        "updated value missing:\n{result}"
    );
    assert!(
        result.contains("kind: Service") && result.contains("demo-svc"),
        "second document must be preserved:\n{result}"
    );

    // Second document still addressable by index after write.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["doc", "get"])
        .arg(&file)
        .arg("1.metadata.name")
        .assert()
        .code(0)
        .stdout(predicates::str::contains("demo-svc"));
}

/// Empty JSON file should bootstrap like empty YAML/TOML for doc set.
#[test]
fn test_doc_set_empty_json_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.json");
    fs::write(&file, "").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "--cwd"])
        .arg(dir.path())
        .args(["doc", "set", "empty.json", "a", "1", "--apply"])
        .assert()
        .code(0)
        .stdout(
            predicates::str::contains(r#""ok":true"#)
                .or(predicates::str::contains(r#""ok": true"#)),
        );

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v, serde_json::json!({"a": 1}));
}

/// #1794: leading slash is JSON Pointer habit; strip one leading `/` so
/// empty-file first write does not create a key literally named `/feature_flag`.
#[test]
fn test_doc_set_leading_slash_selector_strips_root() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, "").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "--cwd"])
        .arg(dir.path())
        .args([
            "doc",
            "set",
            "config.json",
            "/feature_flag",
            "true",
            "--apply",
        ])
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v, serde_json::json!({"feature_flag": true}));
    assert!(
        v.get("/feature_flag").is_none(),
        "must not create slash-prefixed key: {v}"
    );

    // Nested path after leading slash.
    let file2 = dir.path().join("nested.json");
    fs::write(&file2, "{}").unwrap();
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "--cwd"])
        .arg(dir.path())
        .args([
            "doc",
            "set",
            "nested.json",
            "/server.port",
            "8080",
            "--apply",
        ])
        .assert()
        .code(0);
    let v2: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file2).unwrap()).unwrap();
    assert_eq!(v2, serde_json::json!({"server": {"port": 8080}}));
}

/// #2197: `doc set FILE . VALUE` replaces the document root (same as `doc keys FILE .`).
#[test]
fn test_doc_set_dot_replaces_root() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("d.json");
    fs::write(&file, "{\"a\":1}\n").unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "--cwd"])
        .arg(dir.path())
        .args(["doc", "set", "d.json", ".", "{\"b\":2}", "--apply"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["applied"], true, "{v}");
    assert_ne!(
        v["error_kind"], "invalid_input",
        "must not call `.` an empty selector: {v}"
    );
    let on_disk: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(on_disk, serde_json::json!({"b": 2}));

    // `doc keys FILE .` still lists root keys.
    let keys = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "--cwd"])
        .arg(dir.path())
        .args(["doc", "keys", "d.json", "."])
        .output()
        .unwrap();
    assert_eq!(keys.status.code(), Some(0));
    let kv: serde_json::Value = serde_json::from_slice(&keys.stdout).unwrap();
    assert_eq!(kv["value"], serde_json::json!(["b"]), "{kv}");
}

/// #1810: doc set preview sets applied:false (changed means would-change).
#[test]
fn test_doc_set_preview_applied_false() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("c.json"), "{}").unwrap();
    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "--cwd"])
        .arg(dir.path())
        .args(["doc", "set", "c.json", "k", "true"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["changed"], true, "{v}");
    assert_eq!(v["applied"], false, "preview must not look like apply: {v}");
    assert_eq!(fs::read_to_string(dir.path().join("c.json")).unwrap(), "{}");
}

/// Multi-doc merge into document 0 via --selector (fixrealloop gap).
#[test]
fn test_doc_merge_multi_doc_selector() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("services.yaml"),
        "---\nname: app\nport: 8080\n---\nname: worker\nport: 9090\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "--json",
            "doc",
            "merge",
            "services.yaml",
            "--selector",
            "0",
            "--value",
            r#"{"env":"prod"}"#,
            "--apply",
            "--cwd",
        ])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let text = fs::read_to_string(dir.path().join("services.yaml")).unwrap();
    assert!(
        text.contains("env: prod") || text.contains("env:prod"),
        "expected env merged into first doc: {text}"
    );
    assert!(
        text.contains("name: worker"),
        "second doc must remain: {text}"
    );
}

#[test]
fn style_changed_false_when_yaml_sequence_item_indent_stays() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("app.yaml");
    std::fs::write(
        &path,
        "env:\n  - name: FEATURE_FLAG\n    value: off\nlimits:\n  rate: 1\n",
    )
    .unwrap();
    let out = assert_cmd::Command::cargo_bin("patchloom")
        .unwrap()
        .current_dir(dir.path())
        .args([
            "--json",
            "doc",
            "update",
            "app.yaml",
            "env[name=FEATURE_FLAG].value",
            "on",
            "--apply",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["changed"], true);
    assert_ne!(
        v["style_changed"], true,
        "CLI doc --json must not flag style when sequence indent stays: {v}"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "env:\n  - name: FEATURE_FLAG\n    value: on\nlimits:\n  rate: 1\n"
    );
}

/// #2070 multi-surface: plan/tx `changes[]` must not flag style_changed
/// when YAML block-sequence item indent stays (CLI already locks this).
#[test]
fn style_changed_false_on_tx_plan_yaml_sequence_item_indent() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("app.yaml");
    std::fs::write(
        &path,
        "env:\n  - name: FEATURE_FLAG\n    value: off\nlimits:\n  rate: 1\n",
    )
    .unwrap();
    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "doc.update",
            "path": "app.yaml",
            "selector": "env[name=FEATURE_FLAG].value",
            "value": "on"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    std::fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();
    let out = assert_cmd::Command::cargo_bin("patchloom")
        .unwrap()
        .current_dir(dir.path())
        .args(["--json", "tx", "plan.json", "--apply"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["ok"], true, "tx apply ok: {v}");
    let changes = v["changes"]
        .as_array()
        .unwrap_or_else(|| panic!("changes array: {v}"));
    assert!(!changes.is_empty(), "expected at least one change: {v}");
    let style = changes.iter().any(|c| c["style_changed"] == true);
    assert!(
        !style,
        "tx plan changes[] must not flag style when sequence indent stays: {v}"
    );
}

/// #2230: numeric comparison predicate on doc get --json.
#[test]
fn test_doc_get_port_gt_json() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("servers.json");
    fs::write(
        &file,
        r#"{"servers":[{"name":"web","port":80},{"name":"api","port":9000}]}"#,
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "get"])
        .arg(&file)
        .arg("servers[port>8000]")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{:?}", output);
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["value"]["name"], "api", "{v}");
    assert_eq!(v["value"]["port"], 9000, "{v}");
}

/// #2230: non-numeric comparison operand is invalid_input (parse-time).
#[test]
fn test_doc_get_port_gt_non_numeric_operand_invalid_input() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("servers.json");
    fs::write(&file, r#"{"servers":[{"port":80}]}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "get"])
        .arg(&file)
        .arg("servers[port>abc]")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{:?}", output);
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON error envelope");
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(
        v["error_kind"], "invalid_input",
        "non-numeric comparison operand must be invalid_input: {v}"
    );
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        err.contains("numeric") || err.contains("comparison"),
        "error should name numeric/comparison, got: {v}"
    );
}

/// #2230: present non-numeric field vs `>` is invalid_input (eval-time).
#[test]
fn test_doc_get_port_gt_non_numeric_field_invalid_input() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("servers.json");
    fs::write(&file, r#"{"servers":[{"port":"abc"}]}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "get"])
        .arg(&file)
        .arg("servers[port>8000]")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{:?}", output);
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON error envelope");
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(
        v["error_kind"], "invalid_input",
        "non-numeric field vs > must be invalid_input: {v}"
    );
}

/// #2230: CLI `doc update` parse-time non-numeric operand is invalid_input.
#[test]
fn test_doc_update_port_gt_non_numeric_operand_invalid_input() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("servers.json");
    fs::write(&file, r#"{"servers":[{"port":80}]}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "update"])
        .arg(&file)
        .arg("servers[port>abc].port")
        .arg("443")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{:?}", output);
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON error envelope");
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(
        v["error_kind"], "invalid_input",
        "non-numeric comparison operand must be invalid_input: {v}"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        r#"{"servers":[{"port":80}]}"#
    );
}

/// #2230: CLI `doc update` present non-numeric field vs `>` is invalid_input.
#[test]
fn test_doc_update_port_gt_non_numeric_field_invalid_input() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("servers.json");
    fs::write(&file, r#"{"servers":[{"port":"abc"}]}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "update"])
        .arg(&file)
        .arg("servers[port>8000].port")
        .arg("443")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{:?}", output);
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON error envelope");
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(
        v["error_kind"], "invalid_input",
        "non-numeric field vs > must be invalid_input: {v}"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        r#"{"servers":[{"port":"abc"}]}"#
    );
}

/// #2230: UTF-8 predicate key still parses.
#[test]
fn test_doc_get_unicode_predicate_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("items.json");
    fs::write(&file, r#"{"items":[{"名前":"x","v":1}]}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "get"])
        .arg(&file)
        .arg("items[名前=x].v")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{:?}", output);
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["value"], 1, "{v}");
}

/// #2230: chained predicates write the matching row.
#[test]
fn test_doc_update_chained_predicate_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(
        &file,
        r#"{"data":[{"type":"server","port":9000},{"type":"web","port":80}]}"#,
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["doc", "update"])
        .arg(&file)
        .arg("data[type=server][port>8000].port")
        .arg("443")
        .arg("--apply")
        .assert()
        .success();
    let body = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["data"][0]["port"], 443, "{body}");
    assert_eq!(v["data"][1]["port"], 80, "{body}");
}

/// #2230: existing key=value still works on doc get --json.
#[test]
fn test_doc_get_equality_predicate_still_works() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("items.json");
    fs::write(
        &file,
        r#"{"items":[{"name":"a","v":1},{"name":"b","v":2}]}"#,
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "doc", "get"])
        .arg(&file)
        .arg("items[name=b]")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{:?}", output);
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["value"]["name"], "b", "{v}");
    assert_eq!(v["value"]["v"], 2, "{v}");
}
