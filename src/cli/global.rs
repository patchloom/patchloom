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

/// Flags available to all subcommands (read and write).
///
/// Write-only flags are `#[clap(skip)]` here so they don't appear in
/// read-only subcommand help. Write commands flatten [`WriteFlags`]
/// separately and the dispatcher merges them via [`GlobalFlags::merge_write`].
#[derive(Debug, Default, Args)]
pub struct GlobalFlags {
    /// Emit machine-readable JSON output.
    #[arg(long, global = true)]
    pub json: bool,

    /// Emit one JSON object per result line.
    #[arg(long, global = true)]
    pub jsonl: bool,

    /// Suppress non-JSON human-readable output.
    #[arg(long, short = 'q', global = true)]
    pub quiet: bool,

    /// Set working directory.
    #[arg(long, global = true)]
    pub cwd: Option<String>,

    /// Restrict target files by glob pattern (may be repeated).
    #[arg(long, global = true, action = clap::ArgAction::Append)]
    pub glob: Vec<String>,

    /// Read file list from a file or stdin (`-`), one path per line.
    #[arg(long, global = true)]
    pub files_from: Option<String>,

    // -- Write-only flags (populated via merge_write in dispatch) -----------
    #[clap(skip)]
    pub diff: bool,
    #[clap(skip)]
    pub apply: bool,
    #[clap(skip)]
    pub check: bool,
    #[clap(skip)]
    pub ensure_final_newline: bool,
    #[clap(skip)]
    pub normalize_eol: Option<EolMode>,
    #[clap(skip)]
    pub trim_trailing_whitespace: bool,
    #[clap(skip)]
    pub respect_editorconfig: bool,
}

/// Write-only flags exposed in subcommands that mutate files.
///
/// Use `global = true` so these propagate into nested subcommands
/// (e.g. `doc set`, `patch apply`). They only appear in help when
/// the parent command flattens this struct.
#[derive(Debug, Default, Args)]
pub struct WriteFlags {
    /// Print unified diff for any write operation.
    #[arg(long, global = true)]
    pub diff: bool,

    /// Actually mutate files.
    #[arg(long, global = true)]
    pub apply: bool,

    /// Compute and report changes without writing.
    #[arg(long, global = true)]
    pub check: bool,

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
    /// Copy write-only flags from a [`WriteFlags`] into this struct.
    pub fn merge_write(&mut self, w: &WriteFlags) {
        self.diff = w.diff;
        self.apply = w.apply;
        self.check = w.check;
        self.ensure_final_newline = w.ensure_final_newline;
        self.normalize_eol = w.normalize_eol;
        self.trim_trailing_whitespace = w.trim_trailing_whitespace;
        self.respect_editorconfig = w.respect_editorconfig;
    }

    /// Resolve the working directory from `--cwd`, defaulting to the process
    /// current directory.
    pub fn resolve_cwd(&self) -> anyhow::Result<std::path::PathBuf> {
        if let Some(ref cwd) = self.cwd {
            Ok(std::path::PathBuf::from(cwd))
        } else {
            std::env::current_dir().map_err(Into::into)
        }
    }

    /// Read file paths from `--files-from`. Returns `None` if the flag is not set.
    /// When the value is `-`, reads from stdin (one path per line).
    pub fn read_files_from(&self) -> anyhow::Result<Option<Vec<String>>> {
        let source = match self.files_from.as_deref() {
            Some(s) => s,
            None => return Ok(None),
        };
        let lines: Vec<String> = if source == "-" {
            std::io::stdin()
                .lock()
                .lines()
                .map_while(Result::ok)
                .filter(|l| !l.is_empty())
                .collect()
        } else {
            std::fs::read_to_string(source)
                .map_err(|e| anyhow::anyhow!("failed to read --files-from '{}': {e}", source))?
                .lines()
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect()
        };
        Ok(Some(lines))
    }
}
