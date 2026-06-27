#[cfg(feature = "cli")]
use clap::Args;
use std::io::BufRead;

/// Color mode for terminal output.
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum ColorMode {
    /// Auto-detect: color when stdout is a terminal.
    #[default]
    Auto,
    /// Always emit ANSI color codes.
    Always,
    /// Never emit ANSI color codes.
    Never,
}

/// Write policy for EOL normalization. Re-exported from `write` so that
/// CLI code can continue to use `crate::cli::global::EolMode`.
pub use crate::write::EolMode;

/// Flags available to all subcommands (read and write).
///
/// Write-only flags are `#[clap(skip)]` here so they don't appear in
/// read-only subcommand help. Write commands flatten [`WriteFlags`]
/// separately and the dispatcher merges them via [`GlobalFlags::merge_write`].
#[derive(Debug, Default)]
#[cfg_attr(feature = "cli", derive(Args))]
pub struct GlobalFlags {
    /// Emit machine-readable JSON output.
    #[cfg_attr(feature = "cli", arg(long, global = true))]
    pub json: bool,

    /// Emit one JSON object per result line.
    #[cfg_attr(feature = "cli", arg(long, global = true))]
    pub jsonl: bool,

    /// Suppress non-JSON human-readable output.
    #[cfg_attr(feature = "cli", arg(long, short = 'q', global = true))]
    pub quiet: bool,

    /// Enable verbose diagnostic output on stderr for debugging.
    /// Can also be enabled via the PATCHLOOM_LOG environment variable.
    #[cfg_attr(feature = "cli", arg(long, global = true))]
    pub verbose: bool,

    /// Set working directory.
    #[cfg_attr(feature = "cli", arg(long, global = true))]
    pub cwd: Option<String>,

    /// Restrict target files by glob pattern (may be repeated).
    #[cfg_attr(feature = "cli", arg(long, global = true, action = clap::ArgAction::Append))]
    pub glob: Vec<String>,

    /// Exclude paths matching these glob patterns (in addition to .gitignore and custom ignore files). May be repeated. Enables layered excludes (e.g. .blineignore use cases) for search, replace, tidy etc.
    #[cfg_attr(feature = "cli", arg(long, global = true, action = clap::ArgAction::Append))]
    pub exclude: Vec<String>,

    /// Additional gitignore-style ignore filenames to respect (e.g. `.blineignore`). May be repeated. These are processed in addition to .gitignore.
    #[cfg_attr(feature = "cli", arg(long, global = true, action = clap::ArgAction::Append, value_name = "FILENAME"))]
    pub ignore_file: Vec<String>,

    /// Read file list from a file or stdin (`-`), one path per line.
    #[cfg_attr(feature = "cli", arg(long, global = true))]
    pub files_from: Option<String>,

    /// When to use color: auto (default), always, or never.
    #[cfg_attr(
        feature = "cli",
        arg(long, global = true, value_enum, default_value = "auto")
    )]
    pub color: ColorMode,

    // -- Write-only flags (populated via merge_write in dispatch) -----------
    #[cfg_attr(feature = "cli", clap(skip))]
    pub diff: bool,
    #[cfg_attr(feature = "cli", clap(skip))]
    pub apply: bool,
    #[cfg_attr(feature = "cli", clap(skip))]
    pub check: bool,
    #[cfg_attr(feature = "cli", clap(skip))]
    pub ensure_final_newline: bool,
    #[cfg_attr(feature = "cli", clap(skip))]
    pub normalize_eol: Option<EolMode>,
    #[cfg_attr(feature = "cli", clap(skip))]
    pub trim_trailing_whitespace: bool,
    #[cfg_attr(feature = "cli", clap(skip))]
    pub respect_editorconfig: bool,
    #[cfg_attr(feature = "cli", clap(skip))]
    pub collapse_blanks: bool,
    #[cfg_attr(feature = "cli", clap(skip))]
    pub confirm: bool,
    #[cfg_attr(feature = "cli", clap(skip))]
    pub format: Option<String>,
    #[cfg_attr(feature = "cli", clap(skip))]
    pub format_timeout: Option<u64>,
    #[cfg_attr(feature = "cli", clap(skip))]
    pub no_format: bool,
}

/// Write-only flags exposed in subcommands that mutate files.
///
/// Use `global = true` so these propagate into nested subcommands
/// (e.g. `doc set`, `patch apply`). They only appear in help when
/// the parent command flattens this struct.
#[derive(Debug, Default)]
#[cfg_attr(feature = "cli", derive(Args))]
pub struct WriteFlags {
    /// Print unified diff for any write operation.
    #[cfg_attr(feature = "cli", arg(long, global = true))]
    pub diff: bool,

    /// Actually mutate files.
    #[cfg_attr(feature = "cli", arg(long, global = true, conflicts_with = "check"))]
    pub apply: bool,

    /// Compute and report changes without writing.
    #[cfg_attr(feature = "cli", arg(long, global = true, conflicts_with = "apply"))]
    pub check: bool,

    /// Ensure non-empty written files end with a newline.
    #[cfg_attr(feature = "cli", arg(long, global = true))]
    pub ensure_final_newline: bool,

    /// Normalize line endings after write.
    #[cfg_attr(feature = "cli", arg(long, global = true, value_enum))]
    pub normalize_eol: Option<EolMode>,

    /// Remove trailing whitespace on touched lines.
    #[cfg_attr(feature = "cli", arg(long, global = true))]
    pub trim_trailing_whitespace: bool,

    /// Read write policy from .editorconfig when present.
    #[cfg_attr(feature = "cli", arg(long, global = true))]
    pub respect_editorconfig: bool,

    /// Collapse consecutive blank lines into a single blank line after writing.
    #[cfg_attr(feature = "cli", arg(long, global = true))]
    pub collapse_blanks: bool,

    /// Show diff then prompt before applying. Implies --apply on confirmation.
    #[cfg_attr(feature = "cli", arg(long, global = true, conflicts_with_all = ["apply", "check"]))]
    pub confirm: bool,

    /// Run a shell command after successful --apply (e.g. "cargo fmt --all").
    /// Ignored in --diff and --check modes.
    #[cfg_attr(feature = "cli", arg(long, global = true))]
    pub format: Option<String>,

    /// Timeout in seconds for the --format command (default: 30).
    #[cfg_attr(feature = "cli", arg(long, global = true, default_value = "30"))]
    pub format_timeout: Option<u64>,

    /// Disable post-write formatting even if configured in .patchloom.toml.
    #[cfg_attr(feature = "cli", arg(long, global = true))]
    pub no_format: bool,
}

pub(crate) fn confirm_prompt(prompt: &str) -> bool {
    // Flush stdout before the prompt writes to stderr, otherwise the diff
    // output may remain buffered and invisible in a PTY.
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let is_tty = std::io::IsTerminal::is_terminal(&std::io::stdin());
    confirm_prompt_interactive(prompt, is_tty, &mut std::io::stdin().lock())
}

/// Prompt for confirmation when stdin is an interactive TTY.
///
/// Returns `false` immediately when `is_tty` is false (safe fallback for
/// pipes, CI, and `cargo test` without a pseudo-TTY).
pub(crate) fn confirm_prompt_interactive(
    prompt: &str,
    is_tty: bool,
    reader: &mut impl BufRead,
) -> bool {
    if !is_tty {
        return false;
    }
    eprint!("{prompt} [Y/n] ");
    let mut buf = String::new();
    match reader.read_line(&mut buf) {
        Ok(0) | Err(_) => false,
        Ok(_) => {
            let answer = buf.trim().to_lowercase();
            answer.is_empty() || answer == "y" || answer == "yes"
        }
    }
}

impl GlobalFlags {
    /// Copy write-only flags from a [`WriteFlags`] into this struct.
    ///
    /// Uses destructuring so the compiler errors if a new field is added to
    /// [`WriteFlags`] without updating this function.
    pub fn merge_write(&mut self, w: &WriteFlags) {
        let WriteFlags {
            diff,
            apply,
            check,
            confirm,
            ensure_final_newline,
            normalize_eol,
            trim_trailing_whitespace,
            respect_editorconfig,
            collapse_blanks,
            ref format,
            format_timeout,
            no_format,
        } = *w;
        self.diff = diff;
        self.apply = apply;
        self.check = check;
        self.confirm = confirm;
        self.ensure_final_newline = ensure_final_newline;
        self.normalize_eol = normalize_eol;
        self.trim_trailing_whitespace = trim_trailing_whitespace;
        self.respect_editorconfig = respect_editorconfig;
        self.collapse_blanks = collapse_blanks;
        self.format = format.clone();
        self.format_timeout = format_timeout;
        self.no_format = no_format;
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
                #[cfg(feature = "cli")]
                {
                    anstream::stdout().is_terminal()
                }
                #[cfg(not(feature = "cli"))]
                {
                    false
                }
            }
        }
    }

    /// Whether to show human-friendly status messages on stderr.
    ///
    /// Returns true when stderr is a TTY and `--quiet`/`--json`/`--jsonl` are
    /// not set. This lets commands print brief summaries for humans without
    /// interfering with machine-readable stdout.
    pub fn show_status(&self) -> bool {
        !self.quiet && !self.json && !self.jsonl && {
            #[cfg(feature = "cli")]
            {
                anstream::stderr().is_terminal()
            }
            #[cfg(not(feature = "cli"))]
            {
                false
            }
        }
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

    /// Emit a collection of items in structured format.
    ///
    /// In `--json` mode, emits a pretty-printed JSON array of all items.
    /// In `--jsonl` mode, emits one compact JSON line per item.
    /// Returns `true` if structured output was emitted.
    pub fn emit_json_items<T: serde::Serialize>(&self, items: &[T]) -> anyhow::Result<bool> {
        if self.json {
            println!("{}", serde_json::to_string_pretty(items)?);
            Ok(true)
        } else if self.jsonl {
            for item in items {
                println!("{}", serde_json::to_string(item)?);
            }
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
impl GlobalFlags {
    /// Test helper returning GlobalFlags with color=Never for deterministic
    /// test output (avoids TTY/NO_COLOR variance).
    pub fn test_default() -> Self {
        GlobalFlags {
            color: ColorMode::Never,
            ..GlobalFlags::default()
        }
    }

    /// Test helper with cwd set (simulates --cwd for cmd unit tests).
    /// Builds on with_cwd and forces color=Never for test determinism.
    #[allow(clippy::field_reassign_with_default)]
    pub fn test_with_cwd(dir: &std::path::Path) -> Self {
        let mut g = Self::with_cwd(dir);
        g.color = ColorMode::Never;
        g
    }

    /// Test helper with apply=true (for write cmd tests that want mutation).
    pub fn test_apply() -> Self {
        let mut g = Self::test_default();
        g.apply = true;
        g
    }
}

/// Non-test helpers for common construction patterns (reduces boilerplate
/// in MCP, tx, and test code after centralization).
impl GlobalFlags {
    /// Create GlobalFlags with a specific cwd (used by MCP tools and tx paths
    /// to simulate the working directory).
    pub fn with_cwd(cwd: impl AsRef<std::path::Path>) -> Self {
        GlobalFlags {
            cwd: Some(cwd.as_ref().to_string_lossy().into_owned()),
            ..Default::default()
        }
    }

    /// Create GlobalFlags with cwd and json=true (common for MCP read tools).
    pub fn with_cwd_and_json(cwd: impl AsRef<std::path::Path>) -> Self {
        GlobalFlags {
            cwd: Some(cwd.as_ref().to_string_lossy().into_owned()),
            json: true,
            ..Default::default()
        }
    }

    /// Create GlobalFlags with cwd and jsonl=true (for line-delimited JSON in MCP/tx tests).
    pub fn with_cwd_and_jsonl(cwd: impl AsRef<std::path::Path>) -> Self {
        GlobalFlags {
            cwd: Some(cwd.as_ref().to_string_lossy().into_owned()),
            jsonl: true,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_apply_returns_true_when_apply_set() {
        let g = GlobalFlags::test_apply();
        assert!(g.should_apply());
    }

    #[test]
    fn should_apply_returns_false_by_default() {
        let g = GlobalFlags::test_default();
        assert!(!g.should_apply());
    }

    #[test]
    fn confirm_prompt_non_tty_returns_false() {
        let mut reader = std::io::Cursor::new(b"y\n");
        assert!(!confirm_prompt_interactive("Apply?", false, &mut reader));
    }

    #[test]
    fn confirm_prompt_tty_accepts_default_yes() {
        let mut reader = std::io::Cursor::new(b"\n");
        assert!(confirm_prompt_interactive("Apply?", true, &mut reader));
    }

    #[test]
    fn should_apply_confirm_non_tty_returns_false() {
        if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            // Docker and devcontainers often allocate a pseudo-TTY on stdin.
            // Non-TTY behavior is covered by confirm_prompt_non_tty_returns_false.
            return;
        }
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

    /// Verify merge_write copies every field. If a new field is added to
    /// WriteFlags, this test (and the destructuring in merge_write) will
    /// fail to compile until updated.
    #[test]
    fn merge_write_copies_all_fields() {
        let w = WriteFlags {
            diff: true,
            apply: true,
            check: false,
            confirm: true,
            ensure_final_newline: true,
            normalize_eol: Some(EolMode::Lf),
            trim_trailing_whitespace: true,
            respect_editorconfig: true,
            collapse_blanks: true,
            format: Some("cargo fmt".into()),
            format_timeout: Some(60),
            no_format: true,
        };
        let mut g = GlobalFlags::default();
        g.merge_write(&w);
        assert!(g.diff);
        assert!(g.apply);
        assert!(!g.check);
        assert!(g.confirm);
        assert!(g.ensure_final_newline);
        assert_eq!(g.normalize_eol, Some(EolMode::Lf));
        assert!(g.trim_trailing_whitespace);
        assert!(g.respect_editorconfig);
        assert!(g.collapse_blanks);
        assert_eq!(g.format.as_deref(), Some("cargo fmt"));
        assert_eq!(g.format_timeout, Some(60));
        assert!(g.no_format);
    }

    #[test]
    fn verbose_flag_defaults_to_false() {
        let g = GlobalFlags::default();
        assert!(!g.verbose);
    }

    #[test]
    fn resolve_cwd_nonexistent_errors() {
        let g = GlobalFlags {
            cwd: Some("/nonexistent/path/that/does/not/exist".into()),
            ..GlobalFlags::default()
        };
        let err = g.resolve_cwd().unwrap_err().to_string();
        assert!(err.contains("does not exist"), "unexpected: {err}");
    }

    #[test]
    fn resolve_cwd_not_a_directory_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        std::fs::write(&file, "x").unwrap();
        let g = GlobalFlags {
            cwd: Some(file.to_string_lossy().into_owned()),
            ..GlobalFlags::default()
        };
        let err = g.resolve_cwd().unwrap_err().to_string();
        assert!(err.contains("not a directory"), "unexpected: {err}");
    }
}
