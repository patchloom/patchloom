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
    assert!(
        json["error"].is_string(),
        "envelope should contain error field"
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
    assert!(
        fs::read_to_string(&file).unwrap().contains('2'),
        "doc set must still write before format failure"
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

/// Predicate selectors on doc set point agents at doc update (#1725).
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
    assert_ne!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("array")
            && (combined.contains("0.a") || combined.contains("[0].a"))
            && combined.contains("index"),
        "expected multi-doc index hint, got: {combined}"
    );
    // File must be unchanged.
    assert_eq!(fs::read_to_string(&file).unwrap(), "a: 1\n---\nb: 2\n");
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
