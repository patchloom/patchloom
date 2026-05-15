use crate::cli::global::GlobalFlags;
use crate::exit;
use clap::Args;

#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// Path of the file to delete.
    #[arg(long)]
    pub file: String,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

pub fn run(args: DeleteArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    std::env::set_current_dir(global.resolve_cwd()?)?;

    let path = std::path::Path::new(&args.file);

    if !path.exists() {
        anyhow::bail!("file not found: {}", args.file);
    }

    if global.check {
        if !global.quiet {
            println!("would delete {}", args.file);
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    if global.apply {
        std::fs::remove_file(path)?;
        if !global.quiet {
            println!("deleted {}", args.file);
        }
        return Ok(exit::SUCCESS);
    }

    // Default: dry-run.
    println!("would delete {}", args.file);
    Ok(exit::SUCCESS)
}
