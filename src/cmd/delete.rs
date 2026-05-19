use crate::cli::global::GlobalFlags;
use crate::exit;
use clap::Args;
use serde::Serialize;

#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// Path of the file to delete.
    #[arg(long)]
    pub file: String,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

#[derive(Debug, Serialize)]
struct DeleteOutput {
    ok: bool,
    path: String,
    applied: bool,
}

pub fn run(args: DeleteArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    let path = cwd.join(&args.file);

    if !path.exists() {
        anyhow::bail!("file not found: {}", args.file);
    }

    if global.check {
        if global.json {
            let output = DeleteOutput {
                ok: true,
                path: args.file.clone(),
                applied: false,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else if !global.quiet {
            println!("would delete {}", args.file);
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    if global.apply {
        std::fs::remove_file(path)?;
        if global.json {
            let output = DeleteOutput {
                ok: true,
                path: args.file.clone(),
                applied: true,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else if !global.quiet {
            println!("deleted {}", args.file);
        }
        return Ok(exit::SUCCESS);
    }

    // Default: dry-run.
    if global.json {
        let output = DeleteOutput {
            ok: true,
            path: args.file.clone(),
            applied: false,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if !global.quiet {
        println!("would delete {}", args.file);
    }
    Ok(exit::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_args(path: &str) -> DeleteArgs {
        DeleteArgs {
            file: path.to_string(),
            write: crate::cli::global::WriteFlags::default(),
        }
    }

    #[test]
    fn dry_run_does_not_remove_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("target.txt");
        std::fs::write(&file, "content").unwrap();

        let code = run(make_args(&file.to_string_lossy()), &GlobalFlags::default()).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(file.exists(), "dry-run should not delete the file");
    }

    #[test]
    fn apply_removes_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("target.txt");
        std::fs::write(&file, "content").unwrap();

        let global = GlobalFlags {
            apply: true,
            ..GlobalFlags::default()
        };
        let code = run(make_args(&file.to_string_lossy()), &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(!file.exists(), "--apply should delete the file");
    }

    #[test]
    fn check_returns_changes_detected() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("target.txt");
        std::fs::write(&file, "content").unwrap();

        let global = GlobalFlags {
            check: true,
            ..GlobalFlags::default()
        };
        let code = run(make_args(&file.to_string_lossy()), &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
        assert!(file.exists(), "--check should not delete the file");
    }

    #[test]
    fn nonexistent_file_returns_error() {
        let result = run(
            make_args("/tmp/nonexistent_patchloom_test_file_xyz.txt"),
            &GlobalFlags::default(),
        );
        assert!(result.is_err());
    }
}
