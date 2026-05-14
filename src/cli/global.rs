use clap::Args;

/// Write policy for EOL normalization.
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum EolMode {
    /// Keep existing line endings.
    #[default]
    Keep,
    /// Normalize to LF.
    Lf,
    /// Normalize to CRLF.
    Crlf,
}

/// Global flags shared by all subcommands.
#[derive(Debug, Args)]
pub struct GlobalFlags {
    /// Emit machine-readable JSON output.
    #[arg(long, global = true)]
    pub json: bool,

    /// Emit one JSON object per result line.
    #[arg(long, global = true)]
    pub jsonl: bool,

    /// Print unified diff for any write operation.
    #[arg(long, global = true)]
    pub diff: bool,

    /// Actually mutate files.
    #[arg(long, global = true)]
    pub apply: bool,

    /// Compute and report changes without writing.
    #[arg(long, global = true)]
    pub check: bool,

    /// Set working directory.
    #[arg(long, global = true)]
    pub cwd: Option<String>,

    /// Restrict target files by glob pattern.
    #[arg(long, global = true)]
    pub glob: Option<String>,

    /// Require all-or-nothing multi-file apply.
    #[arg(long, global = true)]
    pub atomic: bool,

    /// Ensure non-empty written files end with a newline.
    #[arg(long, global = true)]
    pub ensure_final_newline: bool,

    /// Normalize line endings after write.
    #[arg(long, global = true, value_enum)]
    pub normalize_eol: Option<EolMode>,

    /// Remove trailing whitespace on touched lines.
    #[arg(long, global = true)]
    pub trim_trailing_whitespace: bool,
}
