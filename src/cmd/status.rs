use crate::cli::global::GlobalFlags;
use crate::exit;
use clap::Args;
use serde::Serialize;
use std::process;

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Paths to check (defaults to current directory).
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize)]
struct StatusOutput {
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

/// Parse one line of `git status --porcelain=v1` into a category and file path.
fn parse_porcelain_line(line: &str) -> Option<(FileCategory, String)> {
    if line.len() < 4 {
        return None;
    }
    let xy = &line[..2];
    let file = line[3..].to_string();
    let category = match xy.trim() {
        "??" | "A" | "AM" => FileCategory::Created,
        "D" => FileCategory::Deleted,
        _ => FileCategory::Modified,
    };
    Some((category, file))
}

pub fn run(args: StatusArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;

    let mut cmd = process::Command::new("git");
    cmd.current_dir(&cwd)
        .arg("status")
        .arg("--porcelain=v1")
        .arg("--no-renames");
    for path in &args.paths {
        cmd.arg("--").arg(path);
    }
    let output = cmd.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git status failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let glob_matcher = crate::build_glob_matcher(global)?;

    let mut modified = Vec::new();
    let mut created = Vec::new();
    let mut deleted = Vec::new();

    for line in stdout.lines() {
        let (category, file) = match parse_porcelain_line(line) {
            Some(v) => v,
            None => continue,
        };

        if let Some(ref matcher) = glob_matcher
            && !crate::matches_glob(std::path::Path::new(&file), Some(matcher))
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

    if global.json {
        let out = StatusOutput {
            ok: true,
            modified,
            created,
            deleted,
            total_changes,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else if global.jsonl {
        let out = StatusOutput {
            ok: true,
            modified,
            created,
            deleted,
            total_changes,
        };
        println!("{}", serde_json::to_string(&out)?);
    } else if !global.quiet {
        for f in &modified {
            println!("M  {f}");
        }
        for f in &created {
            println!("A  {f}");
        }
        for f in &deleted {
            println!("D  {f}");
        }
        if total_changes > 0 {
            println!("{total_changes} file(s) changed");
        }
    }

    if total_changes > 0 {
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
        let (cat, file) = parse_porcelain_line("?? new.txt").unwrap();
        assert_eq!(cat, FileCategory::Created);
        assert_eq!(file, "new.txt");
    }

    #[test]
    fn parse_staged_new_file() {
        let (cat, file) = parse_porcelain_line("A  staged.txt").unwrap();
        assert_eq!(cat, FileCategory::Created);
        assert_eq!(file, "staged.txt");
    }

    #[test]
    fn parse_staged_and_modified() {
        let (cat, file) = parse_porcelain_line("AM both.txt").unwrap();
        assert_eq!(cat, FileCategory::Created);
        assert_eq!(file, "both.txt");
    }

    #[test]
    fn parse_deleted_file() {
        let (cat, file) = parse_porcelain_line("D  gone.txt").unwrap();
        assert_eq!(cat, FileCategory::Deleted);
        assert_eq!(file, "gone.txt");
    }

    #[test]
    fn parse_modified_file() {
        let (cat, file) = parse_porcelain_line(" M changed.txt").unwrap();
        assert_eq!(cat, FileCategory::Modified);
        assert_eq!(file, "changed.txt");
    }

    #[test]
    fn parse_short_line_returns_none() {
        assert!(parse_porcelain_line("??").is_none());
        assert!(parse_porcelain_line("A").is_none());
        assert!(parse_porcelain_line("").is_none());
    }
}
