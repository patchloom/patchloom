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

pub fn run(args: StatusArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    std::env::set_current_dir(global.resolve_cwd()?)?;

    let mut cmd = process::Command::new("git");
    cmd.arg("status").arg("--porcelain=v1").arg("--no-renames");
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
        if line.len() < 4 {
            continue;
        }
        let xy = &line[..2];
        let file = line[3..].to_string();

        if let Some(ref matcher) = glob_matcher {
            if !crate::matches_glob(std::path::Path::new(&file), Some(matcher)) {
                continue;
            }
        }

        match xy.trim() {
            "??" | "A" | "AM" => created.push(file),
            "D" => deleted.push(file),
            _ => modified.push(file),
        }
    }

    let total_changes = modified.len() + created.len() + deleted.len();

    if global.json {
        let out = StatusOutput {
            ok: true,
            modified: modified.clone(),
            created: created.clone(),
            deleted: deleted.clone(),
            total_changes,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
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
