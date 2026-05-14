use crate::cli::global::GlobalFlags;
use crate::exit;
use crate::selector;
use clap::Args;
use std::path::Path;

#[derive(Debug, Args)]
pub struct DocArgs {
    #[command(subcommand)]
    pub action: DocAction,
}

#[derive(Debug, clap::Subcommand)]
pub enum DocAction {
    /// Read a value at a key path.
    Get { file: String, selector: String },
    /// Check whether a key path exists.
    Has { file: String, selector: String },
    /// List object keys at a path.
    Keys { file: String, selector: String },
    /// Count items in an array or object.
    Len { file: String, selector: String },
    /// Set or create a value at a key path.
    Set {
        file: String,
        selector: String,
        value: String,
    },
    /// Remove a key path.
    Delete { file: String, selector: String },
    /// Merge a partial object from stdin or argument.
    Merge {
        file: String,
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        value: Option<String>,
    },
    /// Append to an array.
    Append {
        file: String,
        selector: String,
        value: String,
    },
    /// Prepend to an array.
    Prepend {
        file: String,
        selector: String,
        value: String,
    },
    /// Filter array items by predicate.
    Select { file: String, selector: String },
    /// Update all matching nodes.
    Update {
        file: String,
        selector: String,
        value: String,
    },
    /// Move or rename a key path.
    Move {
        file: String,
        from: String,
        to: String,
    },
    /// Ensure a value exists (idempotent set).
    Ensure {
        file: String,
        selector: String,
        value: String,
    },
}

// ---------------------------------------------------------------------------
// File format detection & loading
// ---------------------------------------------------------------------------

enum FileFormat {
    Json,
    Yaml,
    Toml,
}

fn detect_format(path: &str) -> anyhow::Result<FileFormat> {
    match Path::new(path).extension().and_then(|e| e.to_str()) {
        Some("json") => Ok(FileFormat::Json),
        Some("yaml" | "yml") => Ok(FileFormat::Yaml),
        Some("toml") => Ok(FileFormat::Toml),
        Some(ext) => anyhow::bail!("unsupported file extension: .{ext}"),
        None => anyhow::bail!("file has no extension"),
    }
}

fn load_file(path: &str) -> anyhow::Result<serde_json::Value> {
    let content = std::fs::read_to_string(path)?;
    let format = detect_format(path)?;
    match format {
        FileFormat::Json => Ok(serde_json::from_str(&content)?),
        FileFormat::Yaml => Ok(serde_yaml::from_str(&content)?),
        FileFormat::Toml => {
            // Parse with DocumentMut, then deserialize to serde_json::Value.
            let _doc: toml_edit::DocumentMut = content
                .parse()
                .map_err(|e: toml_edit::TomlError| anyhow::anyhow!("{e}"))?;
            let value: serde_json::Value = toml_edit::de::from_str(&content)?;
            Ok(value)
        }
    }
}

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

fn format_value(value: &serde_json::Value, json_mode: bool) -> String {
    if json_mode {
        serde_json::to_string_pretty(value).unwrap_or_default()
    } else {
        match value {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Null => "null".to_string(),
            // Compound values (arrays, objects) always render as JSON.
            _ => serde_json::to_string_pretty(value).unwrap_or_default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Core execution (returns output text + exit code for testability)
// ---------------------------------------------------------------------------

fn execute(action: &DocAction, json_mode: bool) -> anyhow::Result<(String, u8)> {
    match action {
        DocAction::Get { file, selector } => {
            let root = load_file(file)?;
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let results = selector::eval(&root, &sel);
            if results.is_empty() {
                return Ok((String::new(), exit::NO_MATCHES));
            }
            let lines: Vec<String> = results.iter().map(|v| format_value(v, json_mode)).collect();
            Ok((lines.join("\n"), exit::SUCCESS))
        }

        DocAction::Has { file, selector } => {
            let root = load_file(file)?;
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let results = selector::eval(&root, &sel);
            let found = !results.is_empty();
            Ok((found.to_string(), exit::SUCCESS))
        }

        DocAction::Keys { file, selector } => {
            let root = load_file(file)?;
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let results = selector::eval(&root, &sel);
            if results.is_empty() {
                return Ok((String::new(), exit::NO_MATCHES));
            }
            match results[0].as_object() {
                Some(obj) => {
                    let keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
                    Ok((keys.join("\n"), exit::SUCCESS))
                }
                None => Ok((String::new(), exit::PARSE_ERROR)),
            }
        }

        DocAction::Len { file, selector } => {
            let root = load_file(file)?;
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let results = selector::eval(&root, &sel);
            if results.is_empty() {
                return Ok((String::new(), exit::NO_MATCHES));
            }
            let target = results[0];
            if let Some(arr) = target.as_array() {
                Ok((arr.len().to_string(), exit::SUCCESS))
            } else if let Some(obj) = target.as_object() {
                Ok((obj.len().to_string(), exit::SUCCESS))
            } else {
                Ok((String::new(), exit::PARSE_ERROR))
            }
        }

        // Write-mode subcommands – stubs.
        DocAction::Set { .. }
        | DocAction::Delete { .. }
        | DocAction::Merge { .. }
        | DocAction::Append { .. }
        | DocAction::Prepend { .. }
        | DocAction::Select { .. }
        | DocAction::Update { .. }
        | DocAction::Move { .. }
        | DocAction::Ensure { .. } => {
            anyhow::bail!("not implemented")
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(args: DocArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let (output, code) = execute(&args.action, global.json)?;
    if !output.is_empty() {
        println!("{output}");
    }
    Ok(code)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper: write a file into a temp directory and return its path.
    fn write_file(dir: &TempDir, name: &str, content: &str) -> String {
        let path = dir.path().join(name);
        fs::write(&path, content).unwrap();
        path.to_str().unwrap().to_string()
    }

    // -- get ----------------------------------------------------------------

    #[test]
    fn get_returns_value_from_json() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello", "count": 42}"#);
        let action = DocAction::Get {
            file: path,
            selector: "name".into(),
        };
        let (output, code) = execute(&action, false).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "hello");
    }

    #[test]
    fn get_returns_value_from_yaml() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.yaml", "name: hello\ncount: 42\n");
        let action = DocAction::Get {
            file: path,
            selector: "name".into(),
        };
        let (output, code) = execute(&action, false).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "hello");
    }

    #[test]
    fn get_returns_value_from_toml() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.toml", "name = \"hello\"\ncount = 42\n");
        let action = DocAction::Get {
            file: path,
            selector: "name".into(),
        };
        let (output, code) = execute(&action, false).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "hello");
    }

    // -- has ----------------------------------------------------------------

    #[test]
    fn has_returns_true_for_existing_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Has {
            file: path,
            selector: "name".into(),
        };
        let (output, code) = execute(&action, false).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "true");
    }

    #[test]
    fn has_returns_false_for_missing_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Has {
            file: path,
            selector: "missing".into(),
        };
        let (output, code) = execute(&action, false).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "false");
    }

    // -- keys ---------------------------------------------------------------

    #[test]
    fn keys_lists_object_keys() {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            &dir,
            "test.json",
            r#"{"scripts": {"build": "tsc", "lint": "eslint", "test": "jest"}}"#,
        );
        let action = DocAction::Keys {
            file: path,
            selector: "scripts".into(),
        };
        let (output, code) = execute(&action, false).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let keys: Vec<&str> = output.split('\n').collect();
        assert_eq!(keys.len(), 3);
        assert!(keys.contains(&"build"));
        assert!(keys.contains(&"lint"));
        assert!(keys.contains(&"test"));
    }

    // -- len ----------------------------------------------------------------

    #[test]
    fn len_counts_array_elements() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"items": [1, 2, 3, 4, 5]}"#);
        let action = DocAction::Len {
            file: path,
            selector: "items".into(),
        };
        let (output, code) = execute(&action, false).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "5");
    }

    // -- missing selector ---------------------------------------------------

    #[test]
    fn get_missing_selector_returns_exit_3() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Get {
            file: path,
            selector: "nonexistent".into(),
        };
        let (output, code) = execute(&action, false).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
        assert!(output.is_empty());
    }
}
