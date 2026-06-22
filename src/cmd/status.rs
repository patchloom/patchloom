use crate::cli::global::GlobalFlags;
use crate::exit;
use anyhow::Context;
use clap::Args;
use serde::Serialize;
use std::process;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom status
  patchloom status --json")]
pub struct StatusArgs {
    /// Paths to check (defaults to current directory).
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StatusOutput {
    ok: bool,
    modified: Vec<String>,
    created: Vec<String>,
    deleted: Vec<String>,
    total_changes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileCategory {
    Created,
    Deleted,
    Modified,
}

/// Parse one NUL-delimited `git status --porcelain=v1 -z` record.
fn parse_porcelain_record(record: &[u8]) -> Option<(FileCategory, String)> {
    if record.len() < 4 {
        return None;
    }
    let xy = std::str::from_utf8(&record[..2]).ok()?;
    let file = String::from_utf8_lossy(&record[3..]).into_owned();
    let category = match xy.trim() {
        "??" | "A" | "AM" => FileCategory::Created,
        "D" => FileCategory::Deleted,
        _ => FileCategory::Modified,
    };
    Some((category, file))
}

/// Collect git status without writing to stdout.
pub(crate) fn collect_status(
    paths: &[String],
    global: &GlobalFlags,
) -> anyhow::Result<StatusOutput> {
    let cwd = global.resolve_cwd()?;

    let mut cmd = process::Command::new("git");
    cmd.current_dir(&cwd)
        .arg("status")
        .arg("--porcelain=v1")
        .arg("--no-renames")
        .arg("--untracked-files=all")
        .arg("-z");
    for path in paths {
        cmd.arg("--").arg(path);
    }
    let output = cmd
        .output()
        .context("failed to run `git status` -- is git installed?")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!(
            "git status failed: {stderr}\nhint: run `git init` first, or run patchloom status from inside an existing git repository"
        );
    }

    let glob_matcher = crate::build_glob_matcher_from_global(global)?;
    let glob_roots = crate::collect_glob_roots_from_global(paths, global, Some(&cwd))?;

    let mut modified = Vec::new();
    let mut created = Vec::new();
    let mut deleted = Vec::new();

    for record in output
        .stdout
        .split(|b| *b == 0)
        .filter(|record| !record.is_empty())
    {
        let (category, file) = match parse_porcelain_record(record) {
            Some(v) => v,
            None => continue,
        };

        let file_path = cwd.join(&file);
        if let Some(ref matcher) = glob_matcher
            && !crate::matches_glob_with_roots(&file_path, Some(matcher), &glob_roots)
        {
            continue;
        }

        match category {
            FileCategory::Created => created.push(file),
            FileCategory::Deleted => deleted.push(file),
            FileCategory::Modified => modified.push(file),
        }
    }

    let total_changes = modified.len() + created.len() + deleted.len();

    Ok(StatusOutput {
        ok: true,
        modified,
        created,
        deleted,
        total_changes,
    })
}

pub fn run(args: StatusArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let out = collect_status(&args.paths, global)?;

    if !global.emit_json(&out)? && !global.quiet {
        for f in &out.modified {
            println!("M  {f}");
        }
        for f in &out.created {
            println!("A  {f}");
        }
        for f in &out.deleted {
            println!("D  {f}");
        }
        if out.total_changes > 0 {
            println!("{} file(s) changed", out.total_changes);
        }
    }

    if out.total_changes > 0 {
        Ok(exit::CHANGES_DETECTED)
    } else {
        Ok(exit::SUCCESS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_untracked_file() {
        let (cat, file) = parse_porcelain_record(b"?? new.txt").unwrap();
        assert_eq!(cat, FileCategory::Created);
        assert_eq!(file, "new.txt");
    }

    #[test]
    fn parse_staged_new_file() {
        let (cat, file) = parse_porcelain_record(b"A  staged.txt").unwrap();
        assert_eq!(cat, FileCategory::Created);
        assert_eq!(file, "staged.txt");
    }

    #[test]
    fn parse_staged_and_modified() {
        let (cat, file) = parse_porcelain_record(b"AM both.txt").unwrap();
        assert_eq!(cat, FileCategory::Created);
        assert_eq!(file, "both.txt");
    }

    #[test]
    fn parse_deleted_file() {
        let (cat, file) = parse_porcelain_record(b"D  gone.txt").unwrap();
        assert_eq!(cat, FileCategory::Deleted);
        assert_eq!(file, "gone.txt");
    }

    #[test]
    fn parse_modified_file() {
        let (cat, file) = parse_porcelain_record(b" M changed.txt").unwrap();
        assert_eq!(cat, FileCategory::Modified);
        assert_eq!(file, "changed.txt");
    }

    #[test]
    fn parse_short_line_returns_none() {
        assert!(parse_porcelain_record(b"??").is_none());
        assert!(parse_porcelain_record(b"A").is_none());
        assert!(parse_porcelain_record(b"").is_none());
    }

    #[test]
    fn parse_filename_with_spaces() {
        let (cat, file) = parse_porcelain_record(b"?? file name.txt").unwrap();
        assert_eq!(cat, FileCategory::Created);
        assert_eq!(file, "file name.txt");
    }
}
