use super::*;

#[test]
fn test_list_files_temp_dir() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "a\n").unwrap();
    fs::create_dir_all(dir.path().join("skip")).unwrap();
    fs::write(dir.path().join("skip/b.txt"), "b\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "--cwd"])
        .arg(dir.path())
        .args(["list-files", "--exclude", "skip/**"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{:?}", output);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true, "{json}");
    let paths = json["paths"]
        .as_array()
        .expect("paths")
        .iter()
        .filter_map(|p| p.as_str())
        .collect::<Vec<_>>();
    assert!(
        paths.iter().any(|p| *p == "a.txt" || p.ends_with("a.txt")),
        "{json}"
    );
    assert!(
        !paths.iter().any(|p| p.contains("skip")),
        "exclude must drop skip/: {json}"
    );
}

#[test]
fn test_list_files_missing_root_is_not_found() {
    let dir = TempDir::new().unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "--cwd"])
        .arg(dir.path())
        .args(["list-files", "no-such-root"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{:?}", output);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false, "{json}");
    assert_eq!(json["error_kind"], "not_found", "{json}");
}
