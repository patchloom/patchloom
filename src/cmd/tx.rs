use crate::cli::global::{EolMode, GlobalFlags};
use crate::cmd::doc::{deep_merge, detect_format, navigate_mut, parse_doc, serialize_value};
use crate::diff::{format_diff_result, unified_diff, DiffResult};
use crate::exit;
use crate::plan::{self, Operation, Plan};
use crate::selector;
use crate::write::{apply_policy, atomic_write, WritePolicy};
use clap::Args;
use globset::Glob;
use ignore::WalkBuilder;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Args)]
pub struct TxArgs {
    /// Path to a plan JSON file, or `-` for stdin.
    #[arg(long)]
    pub plan: String,
}

// ---------------------------------------------------------------------------
// Markdown helpers (adapted from cmd/md.rs)
// ---------------------------------------------------------------------------

/// Short label for an operation, used in error messages.
fn op_label(op: &Operation) -> &'static str {
    match op {
        Operation::Replace { .. } => "replace",
        Operation::DocSet { .. } => "doc.set",
        Operation::DocDelete { .. } => "doc.delete",
        Operation::DocMerge { .. } => "doc.merge",
        Operation::DocAppend { .. } => "doc.append",
        Operation::MdReplaceSection { .. } => "md.replace_section",
        Operation::MdInsertAfterHeading { .. } => "md.insert_after_heading",
        Operation::HygieneFix { .. } => "hygiene.fix",
        Operation::FileCreate { .. } => "file.create",
        Operation::FileDelete { .. } => "file.delete",
    }
}

fn replace_section_in(content: &str, heading: &str, replacement: &str) -> Option<String> {
    let (body_start, body_end) = crate::cmd::md::find_section(content, heading)?;
    let mut out = String::with_capacity(content.len());
    out.push_str(&content[..body_start]);
    if !replacement.is_empty() {
        out.push_str(replacement);
        if !replacement.ends_with('\n') {
            out.push('\n');
        }
    }
    out.push_str(&content[body_end..]);
    Some(out)
}

fn insert_after_heading_in(content: &str, heading: &str, insertion: &str) -> Option<String> {
    let (body_start, _) = crate::cmd::md::find_section(content, heading)?;
    let mut out = String::with_capacity(content.len() + insertion.len());
    out.push_str(&content[..body_start]);
    out.push_str(insertion);
    if !insertion.is_empty() && !insertion.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&content[body_start..]);
    Some(out)
}

// ---------------------------------------------------------------------------
// Pending file changes
// ---------------------------------------------------------------------------

/// Read file content from the pending map or from disk.
fn read_file_content(
    pending: &mut HashMap<PathBuf, (String, String)>,
    path: &Path,
) -> anyhow::Result<String> {
    if let Some((_, current)) = pending.get(path) {
        return Ok(current.clone());
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
    pending.insert(path.to_path_buf(), (content.clone(), content.clone()));
    Ok(content)
}

/// Update the current content for a file in the pending map.
fn update_file_content(
    pending: &mut HashMap<PathBuf, (String, String)>,
    path: &Path,
    new_content: String,
) {
    if let Some((_, ref mut current)) = pending.get_mut(path) {
        *current = new_content;
    }
}

// ---------------------------------------------------------------------------
// String replacement helper
// ---------------------------------------------------------------------------

fn do_replace(content: &str, from: &str, to: &str, compiled_re: Option<&Regex>) -> String {
    if let Some(re) = compiled_re {
        re.replace_all(content, to).to_string()
    } else {
        content.replace(from, to)
    }
}

// ---------------------------------------------------------------------------
// Operation execution
// ---------------------------------------------------------------------------

fn execute_operation(
    op: &Operation,
    pending: &mut HashMap<PathBuf, (String, String)>,
    deletions: &mut HashSet<PathBuf>,
) -> anyhow::Result<()> {
    match op {
        Operation::Replace {
            glob,
            path,
            mode,
            from,
            to,
        } => {
            let compiled_re = if mode.as_deref() == Some("regex") {
                Some(Regex::new(from)?)
            } else {
                None
            };

            if let Some(p) = path {
                let file_path = PathBuf::from(p);
                let content = read_file_content(pending, &file_path)?;
                let replaced = do_replace(&content, from, to, compiled_re.as_ref());
                update_file_content(pending, &file_path, replaced);
            } else if let Some(pattern) = glob {
                let matcher = Glob::new(pattern)?.compile_matcher();
                let walker = WalkBuilder::new(".").build();
                for entry in walker {
                    let entry = match entry {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                        continue;
                    }
                    let file_path = entry.path().to_path_buf();
                    if !matcher.is_match(&file_path)
                        && !file_path.file_name().is_some_and(|n| matcher.is_match(n))
                    {
                        continue;
                    }
                    let content = match read_file_content(pending, &file_path) {
                        Ok(c) => c,
                        Err(_) => continue,
                    };
                    let replaced = do_replace(&content, from, to, compiled_re.as_ref());
                    update_file_content(pending, &file_path, replaced);
                }
            } else {
                anyhow::bail!("replace operation requires either 'path' or 'glob'");
            }
        }

        Operation::DocSet { path, key, value } => {
            let file_path = PathBuf::from(path);
            let content = read_file_content(pending, &file_path)?;
            let format = detect_format(path)?;
            let mut root = parse_doc(&content, &format)?;

            let sel = selector::parse(key).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;

            let last = sel
                .last()
                .ok_or_else(|| anyhow::anyhow!("empty selector"))?;
            let parent_path = &sel[..sel.len() - 1];
            let parent = navigate_mut(&mut root, parent_path, true)?;

            match last {
                selector::Segment::Key(k) => {
                    parent
                        .as_object_mut()
                        .ok_or_else(|| anyhow::anyhow!("parent is not an object"))?
                        .insert(k.clone(), value.clone());
                }
                selector::Segment::Index(i) => {
                    let arr = parent
                        .as_array_mut()
                        .ok_or_else(|| anyhow::anyhow!("parent is not an array"))?;
                    if *i < arr.len() {
                        arr[*i] = value.clone();
                    } else {
                        anyhow::bail!("index {} out of bounds (len {})", i, arr.len());
                    }
                }
                _ => anyhow::bail!("cannot set at wildcard/predicate"),
            }

            let new_content = serialize_value(&root, &format)?;
            update_file_content(pending, &file_path, new_content);
        }

        Operation::DocDelete { path, key } => {
            let file_path = PathBuf::from(path);
            let content = read_file_content(pending, &file_path)?;
            let format = detect_format(path)?;
            let mut root = parse_doc(&content, &format)?;

            let sel = selector::parse(key).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;

            if sel.is_empty() {
                return Ok(());
            }

            let last = sel.last().unwrap();
            let parent_path = &sel[..sel.len() - 1];
            let parent = navigate_mut(&mut root, parent_path, false)?;

            match last {
                selector::Segment::Key(k) => {
                    if let Some(obj) = parent.as_object_mut() {
                        obj.remove(k.as_str());
                    }
                }
                selector::Segment::Index(i) => {
                    if let Some(arr) = parent.as_array_mut() {
                        if *i < arr.len() {
                            arr.remove(*i);
                        }
                    }
                }
                _ => anyhow::bail!("cannot delete at wildcard/predicate"),
            }

            let new_content = serialize_value(&root, &format)?;
            update_file_content(pending, &file_path, new_content);
        }

        Operation::DocMerge { path, value } => {
            let file_path = PathBuf::from(path);
            let content = read_file_content(pending, &file_path)?;
            let format = detect_format(path)?;
            let mut root = parse_doc(&content, &format)?;
            deep_merge(&mut root, value);
            let new_content = serialize_value(&root, &format)?;
            update_file_content(pending, &file_path, new_content);
        }

        Operation::DocAppend { path, key, value } => {
            let file_path = PathBuf::from(path);
            let content = read_file_content(pending, &file_path)?;
            let format = detect_format(path)?;
            let mut root = parse_doc(&content, &format)?;

            let sel = selector::parse(key).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;

            let target = navigate_mut(&mut root, &sel, false)?;
            let arr = target
                .as_array_mut()
                .ok_or_else(|| anyhow::anyhow!("target is not an array"))?;
            arr.push(value.clone());

            let new_content = serialize_value(&root, &format)?;
            update_file_content(pending, &file_path, new_content);
        }

        Operation::MdReplaceSection {
            path,
            heading,
            content,
        } => {
            let file_path = PathBuf::from(path);
            let file_content = read_file_content(pending, &file_path)?;
            let new_content = replace_section_in(&file_content, heading, content)
                .ok_or_else(|| anyhow::anyhow!("heading not found: {heading}"))?;
            update_file_content(pending, &file_path, new_content);
        }

        Operation::MdInsertAfterHeading {
            path,
            heading,
            content,
        } => {
            let file_path = PathBuf::from(path);
            let file_content = read_file_content(pending, &file_path)?;
            let new_content = insert_after_heading_in(&file_content, heading, content)
                .ok_or_else(|| anyhow::anyhow!("heading not found: {heading}"))?;
            update_file_content(pending, &file_path, new_content);
        }

        Operation::HygieneFix {
            path,
            ensure_final_newline,
        } => {
            let file_path = PathBuf::from(path);
            let content = read_file_content(pending, &file_path)?;
            let mut new = content;
            if ensure_final_newline.unwrap_or(true) {
                new = crate::write::ensure_final_newline(&new);
            }
            update_file_content(pending, &file_path, new);
        }

        Operation::FileCreate { path, content } => {
            let file_path = PathBuf::from(path);
            if pending.contains_key(&file_path) || file_path.exists() {
                anyhow::bail!("file already exists: {path}");
            }
            pending.insert(file_path, (String::new(), content.clone()));
        }

        Operation::FileDelete { path } => {
            let file_path = PathBuf::from(path);
            let content = read_file_content(pending, &file_path)?;
            // Mark current content as empty so the diff shows full deletion.
            update_file_content(pending, &file_path, String::new());
            // If the file was just created in this plan (original is empty),
            // simply remove it from pending instead of deleting on disk.
            if content.is_empty() {
                pending.remove(&file_path);
            } else {
                deletions.insert(file_path);
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Write policy
// ---------------------------------------------------------------------------

fn build_write_policy(plan: &Plan) -> WritePolicy {
    match &plan.write_policy {
        Some(p) => WritePolicy {
            ensure_final_newline: p.ensure_final_newline.unwrap_or(false),
            normalize_eol: match p.normalize_eol.as_deref() {
                Some("lf") => EolMode::Lf,
                Some("crlf") => EolMode::Crlf,
                _ => EolMode::Keep,
            },
            trim_trailing_whitespace: p.trim_trailing_whitespace.unwrap_or(false),
        },
        None => WritePolicy::default(),
    }
}

// ---------------------------------------------------------------------------
// Diff output helper
// ---------------------------------------------------------------------------

fn print_diffs(changes: &[(PathBuf, String, String)]) {
    let diffs: Vec<_> = changes
        .iter()
        .map(|(p, old, new)| unified_diff(&p.to_string_lossy(), old, new))
        .collect();
    let total = diffs.iter().filter(|d| d.has_changes).count();
    let result = DiffResult {
        diffs,
        total_files_changed: total,
    };
    print!("{}", format_diff_result(&result));
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(args: TxArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    // 1. Read plan from file or stdin.
    let plan_text = if args.plan == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        std::fs::read_to_string(&args.plan)
            .map_err(|e| anyhow::anyhow!("failed to read plan file '{}': {e}", args.plan))?
    };

    // 2. Parse plan JSON.
    let plan = match plan::parse_plan(&plan_text) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("tx: plan parse error: {e}");
            return Ok(exit::PARSE_ERROR);
        }
    };

    // 3. Set working directory (plan.cwd overrides global --cwd).
    if let Some(ref cwd) = plan.cwd {
        std::env::set_current_dir(cwd)?;
    } else if let Some(ref cwd) = global.cwd {
        std::env::set_current_dir(cwd)?;
    }

    // 4. Build write policy from plan.
    let write_policy = build_write_policy(&plan);

    // 5. Execute all operations, collecting changes in memory (no writes).
    let mut pending: HashMap<PathBuf, (String, String)> = HashMap::new();
    let mut deletions: HashSet<PathBuf> = HashSet::new();

    for (i, op) in plan.operations.iter().enumerate() {
        if let Err(e) = execute_operation(op, &mut pending, &mut deletions) {
            eprintln!("tx: operation {} ({}) failed: {e}", i + 1, op_label(op));
            return Ok(exit::ROLLBACK);
        }
    }

    // 6. Apply write policy and collect actual file changes.
    let mut changes: Vec<(PathBuf, String, String)> = Vec::new();
    for (path, (original, current)) in &pending {
        let final_content = apply_policy(current, &write_policy);
        if *original != final_content {
            changes.push((path.clone(), original.clone(), final_content));
        }
    }
    changes.sort_by(|a, b| a.0.cmp(&b.0));

    // 7. Output based on mode.
    if global.check {
        if changes.is_empty() {
            return Ok(exit::SUCCESS);
        }
        println!("{} file(s) would change", changes.len());
        return Ok(exit::CHANGES_DETECTED);
    }

    if global.apply {
        // Write all files atomically (policy already applied).
        let noop_policy = WritePolicy::default();
        for (path, _, new_content) in &changes {
            if deletions.contains(path) {
                std::fs::remove_file(path)?;
            } else {
                // Ensure parent directories exist (needed for file.create).
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() && !parent.exists() {
                        std::fs::create_dir_all(parent)?;
                    }
                }
                atomic_write(path, new_content, &noop_policy)?;
            }
        }

        // Show diffs if --diff flag is set.
        if global.diff && !changes.is_empty() {
            print_diffs(&changes);
        }

        // 8. Run validation steps.
        if let Some(ref validate) = plan.validate {
            for step in validate {
                let output = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&step.cmd)
                    .output()?;

                if !output.status.success() && step.required.unwrap_or(false) {
                    eprintln!("tx: required validation failed: {}", step.cmd);
                    return Ok(exit::VALIDATION_FAILED);
                }
            }
        }

        return Ok(exit::SUCCESS);
    }

    // Default / --diff mode: show unified diffs.
    if !changes.is_empty() {
        print_diffs(&changes);
    }

    Ok(exit::SUCCESS)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn default_global() -> GlobalFlags {
        GlobalFlags::default()
    }

    #[test]
    fn multi_op_plan() {
        let dir = TempDir::new().unwrap();

        // Create test files.
        let txt = dir.path().join("test.txt");
        fs::write(&txt, "hello world\n").unwrap();

        let json_file = dir.path().join("config.json");
        fs::write(&json_file, r#"{"name": "old"}"#).unwrap();

        let no_nl = dir.path().join("no_nl.txt");
        fs::write(&no_nl, "content").unwrap();

        // Build plan with replace + doc.set + hygiene.fix.
        let plan_json = serde_json::json!({
            "operations": [
                {
                    "op": "replace",
                    "path": txt.to_str().unwrap(),
                    "from": "hello",
                    "to": "hi"
                },
                {
                    "op": "doc.set",
                    "path": json_file.to_str().unwrap(),
                    "key": "name",
                    "value": "new"
                },
                {
                    "op": "hygiene.fix",
                    "path": no_nl.to_str().unwrap(),
                    "ensure_final_newline": true
                }
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        // Verify replace.
        assert_eq!(fs::read_to_string(&txt).unwrap(), "hi world\n");

        // Verify doc.set.
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&json_file).unwrap()).unwrap();
        assert_eq!(config["name"], serde_json::json!("new"));

        // Verify hygiene.fix.
        assert!(fs::read_to_string(&no_nl).unwrap().ends_with('\n'));
    }

    #[test]
    fn rollback_on_failure() {
        let dir = TempDir::new().unwrap();

        let txt = dir.path().join("test.txt");
        fs::write(&txt, "hello world\n").unwrap();

        let nonexistent = dir.path().join("nonexistent.json");

        let plan_json = serde_json::json!({
            "operations": [
                {
                    "op": "replace",
                    "path": txt.to_str().unwrap(),
                    "from": "hello",
                    "to": "hi"
                },
                {
                    "op": "doc.set",
                    "path": nonexistent.to_str().unwrap(),
                    "key": "name",
                    "value": "test"
                }
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::ROLLBACK);

        // Verify no files were modified.
        assert_eq!(fs::read_to_string(&txt).unwrap(), "hello world\n");
    }

    #[test]
    fn validation_pass() {
        let dir = TempDir::new().unwrap();

        let plan_json = serde_json::json!({
            "operations": [],
            "validate": [
                {"cmd": "true", "required": true}
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn validation_fail_required() {
        let dir = TempDir::new().unwrap();

        let plan_json = serde_json::json!({
            "operations": [],
            "validate": [
                {"cmd": "false", "required": true}
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::VALIDATION_FAILED);
    }

    #[test]
    fn plan_from_stdin() {
        let plan_json =
            r#"{"operations": [{"op": "replace", "path": "test.txt", "from": "a", "to": "b"}]}"#;
        let plan = plan::parse_plan(plan_json).unwrap();
        assert_eq!(plan.operations.len(), 1);
    }

    #[test]
    fn malformed_plan_returns_parse_error() {
        let dir = TempDir::new().unwrap();

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, "not json at all {{{").unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
        };
        let global = default_global();

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::PARSE_ERROR);
    }

    #[test]
    fn tx_file_create_in_plan() {
        let dir = TempDir::new().unwrap();

        let new_file = dir.path().join("created.txt");

        let plan_json = serde_json::json!({
            "operations": [
                {
                    "op": "file.create",
                    "path": new_file.to_str().unwrap(),
                    "content": "brand new file\n"
                }
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        // Verify the file was created with the correct content.
        assert!(new_file.exists());
        assert_eq!(fs::read_to_string(&new_file).unwrap(), "brand new file\n");
    }

    #[test]
    fn tx_file_create_existing_fails() {
        let dir = TempDir::new().unwrap();

        let existing = dir.path().join("existing.txt");
        fs::write(&existing, "original content\n").unwrap();

        let plan_json = serde_json::json!({
            "operations": [
                {
                    "op": "file.create",
                    "path": existing.to_str().unwrap(),
                    "content": "should not overwrite\n"
                }
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::ROLLBACK);

        // Verify the original file was NOT modified.
        assert_eq!(fs::read_to_string(&existing).unwrap(), "original content\n");
    }
}
