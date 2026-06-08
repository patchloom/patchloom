use clap::Args;
use std::io::BufRead;

/// Color mode for terminal output.
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum ColorMode {
    /// Auto-detect: color when stdout is a terminal.
    #[default]
    Auto,
    /// Always emit ANSI color codes.
    Always,
    /// Never emit ANSI color codes.
    Never,
}

/// Write policy for EOL normalization.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
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

    /// Enable verbose diagnostic output on stderr for debugging.
    /// Can also be enabled via the PATCHLOOM_LOG environment variable.
    #[arg(long, global = true)]
    pub verbose: bool,

    /// Set working directory.
    #[arg(long, global = true)]
    pub cwd: Option<String>,

    /// Restrict target files by glob pattern (may be repeated).
    #[arg(long, global = true, action = clap::ArgAction::Append)]
    pub glob: Vec<String>,

    /// Read file list from a file or stdin (`-`), one path per line.
    #[arg(long, global = true)]
    pub files_from: Option<String>,

    /// When to use color: auto (default), always, or never.
    #[arg(long, global = true, value_enum, default_value = "auto")]
    pub color: ColorMode,

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
    #[clap(skip)]
    pub confirm: bool,
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
    #[arg(long, global = true, conflicts_with = "check")]
    pub apply: bool,

    /// Compute and report changes without writing.
    #[arg(long, global = true, conflicts_with = "apply")]
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

    /// Show diff then prompt before applying. Implies --apply on confirmation.
    #[arg(long, global = true, conflicts_with_all = ["apply", "check"])]
    pub confirm: bool,
}

pub(crate) fn confirm_prompt(prompt: &str) -> bool {
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        return false;
    }
    eprint!("{prompt} [Y/n] ");
    let mut buf = String::new();
    match std::io::stdin().read_line(&mut buf) {
        Ok(0) | Err(_) => false,
        Ok(_) => {
            let answer = buf.trim().to_lowercase();
            answer.is_empty() || answer == "y" || answer == "yes"
        }
    }
}

impl GlobalFlags {
    /// Copy write-only flags from a [`WriteFlags`] into this struct.
    pub fn merge_write(&mut self, w: &WriteFlags) {
        self.diff = w.diff;
        self.apply = w.apply;
        self.check = w.check;
        self.confirm = w.confirm;
        self.ensure_final_newline = w.ensure_final_newline;
        self.normalize_eol = w.normalize_eol;
        self.trim_trailing_whitespace = w.trim_trailing_whitespace;
        self.respect_editorconfig = w.respect_editorconfig;
    }

    /// Whether to proceed with the write after optional confirmation.
    ///
    /// Returns `true` when `--apply` is set, or when `--confirm` is set and
    /// the user answers yes at the interactive prompt. When `--confirm` is
    /// used but stdin is not a TTY, returns `false` (safe fallback).
    pub fn should_apply(&self) -> bool {
        if self.apply {
            return true;
        }
        self.confirm && confirm_prompt("Apply?")
    }

    /// Whether to emit ANSI color codes to stdout.
    ///
    /// Respects `--color`, the `NO_COLOR` env var, and TTY detection.
    pub fn should_color(&self) -> bool {
        match self.color {
            ColorMode::Always => true,
            ColorMode::Never => false,
            ColorMode::Auto => {
                // Respect NO_COLOR (https://no-color.org)
                if std::env::var_os("NO_COLOR").is_some() {
                    return false;
                }
                anstream::stdout().is_terminal()
            }
        }
    }

    /// Whether to show human-friendly status messages on stderr.
    ///
    /// Returns true when stderr is a TTY and `--quiet`/`--json`/`--jsonl` are
    /// not set. This lets commands print brief summaries for humans without
    /// interfering with machine-readable stdout.
    pub fn show_status(&self) -> bool {
        !self.quiet && !self.json && !self.jsonl && anstream::stderr().is_terminal()
    }

    /// Resolve the working directory from `--cwd`, defaulting to the process
    /// current directory.
    pub fn resolve_cwd(&self) -> anyhow::Result<std::path::PathBuf> {
        if let Some(ref cwd) = self.cwd {
            let path = std::path::PathBuf::from(cwd);
            if !path.exists() {
                anyhow::bail!("--cwd directory does not exist: {cwd}");
            }
            if !path.is_dir() {
                anyhow::bail!("--cwd is not a directory: {cwd}");
            }
            Ok(path)
        } else {
            std::env::current_dir().map_err(Into::into)
        }
    }

    /// Emit a serializable value to stdout in structured format.
    ///
    /// If `--json` is set, pretty-prints. If `--jsonl` is set, prints compact.
    /// Returns `true` if structured output was emitted, `false` if the caller
    /// should handle text-mode output (respecting `--quiet`).
    pub fn emit_json<T: serde::Serialize>(&self, value: &T) -> anyhow::Result<bool> {
        if self.json {
            println!("{}", serde_json::to_string_pretty(value)?);
            Ok(true)
        } else if self.jsonl {
            println!("{}", serde_json::to_string(value)?);
            Ok(true)
        } else {
            Ok(false)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_apply_returns_true_when_apply_set() {
        let g = GlobalFlags {
            apply: true,
            ..GlobalFlags::default()
        };
        assert!(g.should_apply());
    }

    #[test]
    fn should_apply_returns_false_by_default() {
        let g = GlobalFlags::default();
        assert!(!g.should_apply());
    }

    #[test]
    fn should_apply_confirm_non_tty_returns_false() {
        // In test environments, stdin is not a TTY, so --confirm should
        // return false (safe fallback).
        let g = GlobalFlags {
            confirm: true,
            ..GlobalFlags::default()
        };
        assert!(!g.should_apply());
    }

    #[test]
    fn merge_write_copies_confirm() {
        let w = WriteFlags {
            confirm: true,
            ..WriteFlags::default()
        };
        let mut g = GlobalFlags::default();
        g.merge_write(&w);
        assert!(g.confirm);
    }

    #[test]
    fn verbose_flag_defaults_to_false() {
        let g = GlobalFlags::default();
        assert!(!g.verbose);
    }
}
