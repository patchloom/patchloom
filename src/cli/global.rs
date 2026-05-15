use clap::Args;
use std::io::BufRead;

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
#[derive(Debug, Default, Args)]
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

    /// Read file list from a file or stdin (`-`), one path per line.
    #[arg(long, global = true)]
    pub files_from: Option<String>,

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

    /// Read write policy from .editorconfig when present.
    #[arg(long, global = true)]
    pub respect_editorconfig: bool,
}

impl GlobalFlags {
    /// Read file paths from `--files-from`. Returns `None` if the flag is not set.
    /// When the value is `-`, reads from stdin (one path per line).
    pub fn read_files_from(&self) -> Option<Vec<String>> {
        let source = self.files_from.as_deref()?;
        let lines: Vec<String> = if source == "-" {
            std::io::stdin()
                .lock()
                .lines()
                .map_while(Result::ok)
                .filter(|l| !l.is_empty())
                .collect()
        } else {
            std::fs::read_to_string(source)
                .unwrap_or_default()
                .lines()
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect()
        };
        Some(lines)
    }
}
