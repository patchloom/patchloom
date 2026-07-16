//! Global CLI flags, path resolution, and `--files-from` list loading.
//!
//! size-waiver: accepted single-domain bulk (policy #1408). One module owns
//! GlobalFlags/WriteFlags, cwd/contain path helpers, and files-from IO;
//! co-located unit tests push the file over 1000 lines. Do not split for LOC alone.

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

    /// Reject paths that escape the working directory (via `../`, absolute
    /// paths that resolve outside the workspace, or outside symlinks) for
    /// reads, writes, and meta-input files (plans, batch ops, patch files,
    /// `--files-from` lists). Absolute paths under the workspace are allowed.
    /// Default CLI mode is unrestricted; use this for agent sandboxes. MCP
    /// always enforces containment and still rejects absolute path strings.
    #[cfg_attr(feature = "cli", arg(long, global = true))]
    pub contain: bool,

    /// Restrict target files by glob pattern (may be repeated).
    #[cfg_attr(feature = "cli", arg(long, global = true, action = clap::ArgAction::Append))]
    pub glob: Vec<String>,

    /// Exclude paths matching these glob patterns (in addition to .gitignore and custom ignore files). May be repeated. Enables layered excludes for search, replace, tidy etc.
    #[cfg_attr(feature = "cli", arg(long, global = true, action = clap::ArgAction::Append))]
    pub exclude: Vec<String>,

    /// Additional gitignore-style ignore filenames to respect (e.g. `.agentignore`). May be repeated. These are processed in addition to .gitignore.
    #[cfg_attr(feature = "cli", arg(long, global = true, action = clap::ArgAction::Append, value_name = "FILENAME"))]
    pub ignore_file: Vec<String>,

    /// Read file list from a file or stdin (`-`), one path per line.
    /// Blank lines and lines whose first non-whitespace character is `#` are
    /// comments and are ignored (#1811).
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

    /// Per-extension format config from `.patchloom.toml`.
    /// Populated by `apply_config`, not by CLI flags.
    #[cfg_attr(feature = "cli", clap(skip))]
    pub format_config: Option<crate::config::FormatConfig>,
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

// Test-only override for confirm_prompt. Docker and many evaluation harnesses
// allocate a pseudo-TTY on stdin, so unit tests that need a deterministic
// decline must not read the real terminal.
#[cfg(test)]
thread_local! {
    static CONFIRM_ANSWER_OVERRIDE: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
}

/// RAII guard that forces a fixed confirm answer on the current thread.
///
/// Restores the previous override when dropped. Production code never sets
/// the override; only tests use this. Needed because Docker/eval harnesses
/// often allocate a pseudo-TTY, which would prompt and default to yes.
#[cfg(test)]
#[doc(hidden)]
pub struct ConfirmAnswerGuard {
    prev: Option<bool>,
}

#[cfg(test)]
impl ConfirmAnswerGuard {
    /// Force `confirm_prompt` / `should_apply` to return this answer.
    pub fn force(answer: bool) -> Self {
        let prev = CONFIRM_ANSWER_OVERRIDE.with(|cell| {
            let previous = cell.get();
            cell.set(Some(answer));
            previous
        });
        Self { prev }
    }
}

#[cfg(test)]
impl Drop for ConfirmAnswerGuard {
    fn drop(&mut self) {
        CONFIRM_ANSWER_OVERRIDE.with(|cell| cell.set(self.prev));
    }
}

pub(crate) fn confirm_prompt(prompt: &str) -> bool {
    #[cfg(test)]
    {
        if let Some(answer) = CONFIRM_ANSWER_OVERRIDE.with(|cell| cell.get()) {
            return answer;
        }
    }
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
                return Err(crate::exit::InvalidInputError {
                    msg: format!("--cwd directory does not exist: {cwd}"),
                }
                .into());
            }
            if !path.is_dir() {
                return Err(crate::exit::InvalidInputError {
                    msg: format!("--cwd is not a directory: {cwd}"),
                }
                .into());
            }
            Ok(path)
        } else {
            std::env::current_dir().map_err(Into::into)
        }
    }

    /// Human-readable scope for "no matches" messages: `--files-from` when set,
    /// otherwise the positional path list (or `.` when empty).
    ///
    /// Without this, `--files-from` runs report "no matches … in ." which is
    /// misleading when the walk never used the workspace root.
    pub fn path_scope_description(&self, paths: &[String]) -> String {
        if let Some(src) = self.files_from.as_deref() {
            if src == "-" {
                return "--files-from -".to_string();
            }
            return format!("--files-from {src}");
        }
        if paths.is_empty() {
            ".".to_string()
        } else {
            paths.join(", ")
        }
    }

    /// Resolve a user-supplied path against [`Self::resolve_cwd`].
    ///
    /// Absolute paths are returned unchanged (`Path::join` replaces the base).
    /// Relative paths become `<cwd>/<path>`. Use for meta-inputs (`tx` plans,
    /// `batch` ops files, `patch` files, `--files-from` lists) so `--cwd` is
    /// consistent across commands.
    ///
    /// When [`Self::contain`] is set, the user-supplied path is also checked
    /// with the workspace [`crate::containment::PathGuard`] (same policy as
    /// operation targets). Absolute paths are allowed only when they resolve
    /// inside the workspace (`AllowIfContained`); escapes and outside
    /// absolutes are rejected. This prevents reading plan/ops/list files from
    /// outside the workspace under `--contain` while still accepting agent
    /// invocations that pass absolute paths under `--cwd`.
    pub fn resolve_user_path(
        &self,
        path: impl AsRef<std::path::Path>,
    ) -> anyhow::Result<std::path::PathBuf> {
        let path = path.as_ref();
        let path_str = path.to_string_lossy();
        if path_str.trim().is_empty() {
            return Err(crate::exit::InvalidInputError {
                msg: "path must not be empty".into(),
            }
            .into());
        }
        let cwd = self.resolve_cwd()?;
        // Always run empty-path + optional --contain checks via the shared helper.
        self.check_paths_contained(&cwd, std::iter::once(path_str.as_ref()))?;
        Ok(cwd.join(path))
    }

    /// Build a workspace [`PathGuard`] when `--contain` is set.
    ///
    /// Uses [`AbsolutePathPolicy::AllowIfContained`]: absolute paths are OK
    /// only if they canonicalize under the workspace root (agent-friendly
    /// sandbox). Relative `../` escapes and absolute paths outside the
    /// workspace are still rejected. MCP keeps its own stricter policy.
    ///
    /// Returns `Ok(None)` when containment is off (default CLI behavior).
    pub fn workspace_guard(
        &self,
        cwd: &std::path::Path,
    ) -> anyhow::Result<Option<crate::containment::PathGuard>> {
        if !self.contain {
            return Ok(None);
        }
        crate::containment::PathGuard::new(
            cwd.to_path_buf(),
            crate::containment::AbsolutePathPolicy::AllowIfContained,
        )
        .map(Some)
        .map_err(|e| {
            anyhow::Error::new(crate::exit::InvalidInputError {
                msg: format!("failed to initialize --contain path guard: {e}"),
            })
        })
    }

    /// Validate user path args and, when `--contain` is set, reject escapes.
    ///
    /// Always rejects empty / whitespace-only path strings (agent footgun:
    /// `ast list ''` and similar join to the workspace root and scan everything).
    /// When containment is off, non-empty paths are not further checked.
    ///
    /// Used by read-side commands (`search`, `read`) and as an early gate on
    /// write commands that scan before staging.
    pub fn check_paths_contained(
        &self,
        cwd: &std::path::Path,
        paths: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> anyhow::Result<()> {
        let paths: Vec<_> = paths.into_iter().collect();
        for p in &paths {
            let p = p.as_ref();
            if p.trim().is_empty() {
                return Err(crate::exit::InvalidInputError {
                    msg: "path must not be empty".into(),
                }
                .into());
            }
        }
        let Some(guard) = self.workspace_guard(cwd)? else {
            return Ok(());
        };
        for p in paths {
            let p = p.as_ref();
            guard.check_path(p).map_err(|e| {
                anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: format!("path rejected by workspace guard: {e}"),
                })
            })?;
        }
        Ok(())
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

    /// Emit a `{"ok": false, "error": msg}` payload in structured format,
    /// with a stderr fallback in text mode (unless `--quiet`).
    ///
    /// Prefer [`Self::emit_error_json_kind`] when the exit path has a stable
    /// machine-readable kind (for example `no_matches` for exit 3). In
    /// JSON/JSONL mode the payload is printed to stdout; in text mode the
    /// message goes to stderr.
    pub fn emit_error_json(&self, msg: &str) -> anyhow::Result<()> {
        self.emit_error_json_kind(None, msg)
    }

    /// Like [`Self::emit_error_json`], but includes `error_kind` when `kind`
    /// is `Some` so agents can branch without scraping stderr (aligned with
    /// tx plan JSON and CLI search/replace soft no-match payloads).
    pub fn emit_error_json_kind(&self, kind: Option<&str>, msg: &str) -> anyhow::Result<()> {
        let mut payload = serde_json::json!({"ok": false, "error": msg});
        if let Some(k) = kind {
            payload["error_kind"] = serde_json::Value::String(k.to_string());
        }
        if !self.emit_json(&payload)? && !self.quiet {
            eprintln!("{msg}");
        }
        Ok(())
    }

    /// Emit the shared agent error envelope for an `anyhow::Error` (kind +
    /// optional `backup_session` for post-write format failures).
    ///
    /// Returns the exit code from [`crate::exit::structured_error_payload`].
    pub fn emit_error_from_anyhow(&self, err: &anyhow::Error) -> anyhow::Result<u8> {
        let (payload, code) = crate::exit::structured_error_payload(err);
        if !self.emit_json(&payload)? && !self.quiet {
            eprintln!("{err}");
        }
        Ok(code)
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
    ///
    /// The list file path is resolved relative to `--cwd` when that flag is set
    /// and the list path is not absolute (so `patchloom --cwd /ws --files-from
    /// list.txt …` opens `/ws/list.txt`, not the process cwd).
    pub fn read_files_from(&self) -> anyhow::Result<Option<Vec<String>>> {
        let source = match self.files_from.as_deref() {
            Some(s) => s,
            None => return Ok(None),
        };
        let lines: Vec<String> = if source == "-" {
            // Propagate IO errors (#1449). Do not use map_while(Result::ok),
            // which silently truncates the list on the first Err.
            collect_files_from_line_results(std::io::stdin().lock().lines())?
        } else {
            let list_path = self.resolve_user_path(source)?;
            let content = std::fs::read_to_string(&list_path).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("failed to read --files-from '{}': {e}", list_path.display()),
                    )
                    .into()
                } else {
                    anyhow::Error::new(crate::exit::InvalidInputError {
                        msg: format!("failed to read --files-from '{}': {e}", list_path.display()),
                    })
                }
            })?;
            let content = content.strip_prefix('\u{FEFF}').unwrap_or(&content);
            content
                .lines()
                .filter_map(normalize_files_from_line)
                .collect()
        };
        Ok(Some(lines))
    }
}

/// Normalize one `--files-from` line: trim, drop blanks and `#` comments (#1811).
///
/// Lines whose first non-whitespace character is `#` are comments (gitignore-
/// style). A path that literally starts with `#` is not supported; document
/// that agents must not emit comment lines if they need such a path.
fn normalize_files_from_line(line: &str) -> Option<String> {
    let l = line.trim();
    if l.is_empty() || l.starts_with('#') {
        None
    } else {
        Some(l.to_string())
    }
}

/// Collect and normalize path lines from an iterator of line-read results.
///
/// Used by `--files-from -` (stdin). IO errors are returned as `Err` so a
/// broken pipe or read failure cannot produce a partial list with success.
fn collect_files_from_line_results(
    lines: impl IntoIterator<Item = std::io::Result<String>>,
) -> anyhow::Result<Vec<String>> {
    let mut out = Vec::new();
    for (i, line_res) in lines.into_iter().enumerate() {
        let mut l = line_res.map_err(|e| {
            anyhow::Error::new(crate::exit::InvalidInputError {
                msg: format!("failed to read --files-from from stdin: {e}"),
            })
        })?;
        // BOM is not Unicode whitespace: strip it before trim on the first line.
        if i == 0
            && let Some(stripped) = l.strip_prefix('\u{FEFF}')
        {
            l = stripped.to_string();
        }
        if let Some(path) = normalize_files_from_line(&l) {
            out.push(path);
        }
    }
    Ok(out)
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
    pub fn test_with_cwd(dir: &std::path::Path) -> Self {
        GlobalFlags {
            color: ColorMode::Never,
            ..Self::with_cwd(dir)
        }
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

    /// Create a copy of `base` with `respect_editorconfig` overridden.
    ///
    /// Used by the tx engine when a plan-level `write_policy` sets
    /// `respect_editorconfig`, since EditorConfig properties must be
    /// resolved during `policy_from_flags` construction, not after (#1111.3).
    pub fn with_editorconfig(base: &GlobalFlags, ec: bool) -> Self {
        GlobalFlags {
            cwd: base.cwd.clone(),
            contain: base.contain,
            json: base.json,
            jsonl: base.jsonl,
            quiet: base.quiet,
            glob: base.glob.clone(),
            exclude: base.exclude.clone(),
            ignore_file: base.ignore_file.clone(),
            files_from: base.files_from.clone(),
            diff: base.diff,
            apply: base.apply,
            check: base.check,
            ensure_final_newline: base.ensure_final_newline,
            normalize_eol: base.normalize_eol,
            trim_trailing_whitespace: base.trim_trailing_whitespace,
            collapse_blanks: base.collapse_blanks,
            respect_editorconfig: ec,
            confirm: base.confirm,
            no_format: base.no_format,
            format: base.format.clone(),
            format_timeout: base.format_timeout,
            format_config: base.format_config.clone(),
            verbose: base.verbose,
            color: base.color,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_json_returns_false_when_neither_flag() {
        let g = GlobalFlags::test_default();
        assert!(!g.emit_json(&serde_json::json!({"a":1})).unwrap());
    }

    #[test]
    fn emit_json_returns_true_for_json_flag() {
        let g = GlobalFlags {
            json: true,
            ..GlobalFlags::test_default()
        };
        assert!(g.emit_json(&serde_json::json!({"a":1})).unwrap());
    }

    #[test]
    fn emit_json_returns_true_for_jsonl_flag() {
        let g = GlobalFlags {
            jsonl: true,
            ..GlobalFlags::test_default()
        };
        assert!(g.emit_json(&serde_json::json!({"a":1})).unwrap());
    }

    #[test]
    fn emit_error_json_does_not_panic_in_text_mode() {
        let g = GlobalFlags::test_default();
        g.emit_error_json("something went wrong").unwrap();
    }

    #[test]
    fn emit_error_json_kind_serializes_kind_field() {
        // Build the same payload shape the helper emits so we lock the contract
        // without capturing stdout in unit tests.
        let mut payload = serde_json::json!({"ok": false, "error": "missing"});
        payload["error_kind"] = serde_json::Value::String("no_matches".into());
        assert_eq!(payload["error_kind"], "no_matches");
        assert_eq!(payload["ok"], false);
        let g = GlobalFlags {
            json: true,
            ..GlobalFlags::test_default()
        };
        g.emit_error_json_kind(Some("no_matches"), "missing")
            .unwrap();
    }

    #[test]
    fn emit_error_json_does_not_panic_in_json_mode() {
        let g = GlobalFlags {
            json: true,
            ..GlobalFlags::test_default()
        };
        g.emit_error_json("something went wrong").unwrap();
    }

    #[test]
    fn emit_error_json_does_not_panic_in_quiet_mode() {
        let g = GlobalFlags {
            quiet: true,
            ..GlobalFlags::test_default()
        };
        g.emit_error_json("something went wrong").unwrap();
    }

    #[test]
    fn emit_json_items_returns_false_when_neither_flag() {
        let g = GlobalFlags::test_default();
        let items: Vec<serde_json::Value> = vec![];
        assert!(!g.emit_json_items(&items).unwrap());
    }

    #[test]
    fn emit_json_items_returns_true_for_json_flag() {
        let g = GlobalFlags {
            json: true,
            ..GlobalFlags::test_default()
        };
        let items = vec![serde_json::json!(1), serde_json::json!(2)];
        assert!(g.emit_json_items(&items).unwrap());
    }

    #[test]
    fn emit_json_items_returns_true_for_jsonl_flag() {
        let g = GlobalFlags {
            jsonl: true,
            ..GlobalFlags::test_default()
        };
        let items = vec![serde_json::json!(1), serde_json::json!(2)];
        assert!(g.emit_json_items(&items).unwrap());
    }

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
    fn confirm_answer_guard_forces_decline_even_with_tty_stdin() {
        // Simulates Docker/eval harnesses that allocate a pseudo-TTY: without
        // the guard, should_apply() would prompt and default to yes on empty.
        let _guard = ConfirmAnswerGuard::force(false);
        let g = GlobalFlags {
            confirm: true,
            ..GlobalFlags::default()
        };
        assert!(!g.should_apply());
        assert!(!confirm_prompt("Apply?"));
    }

    #[test]
    fn confirm_answer_guard_forces_accept() {
        let _guard = ConfirmAnswerGuard::force(true);
        let g = GlobalFlags {
            confirm: true,
            ..GlobalFlags::default()
        };
        assert!(g.should_apply());
    }

    #[test]
    fn should_apply_confirm_non_tty_returns_false() {
        if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            // Docker and devcontainers often allocate a pseudo-TTY on stdin.
            // Non-TTY behavior is covered by confirm_prompt_non_tty_returns_false
            // and ConfirmAnswerGuard unit tests above.
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

    #[test]
    fn resolve_user_path_joins_relative_under_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let g = GlobalFlags {
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            ..GlobalFlags::default()
        };
        let resolved = g.resolve_user_path("ops.txt").unwrap();
        assert_eq!(resolved, dir.path().join("ops.txt"));
    }

    #[test]
    fn resolve_user_path_keeps_absolute() {
        let dir = tempfile::tempdir().unwrap();
        let abs = dir.path().join("abs.txt");
        let g = GlobalFlags {
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            ..GlobalFlags::default()
        };
        let resolved = g.resolve_user_path(&abs).unwrap();
        assert_eq!(resolved, abs);
    }

    #[test]
    fn resolve_user_path_contain_rejects_parent_escape() {
        let dir = tempfile::tempdir().unwrap();
        let g = GlobalFlags {
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            contain: true,
            ..GlobalFlags::default()
        };
        let err = g
            .resolve_user_path("../outside.txt")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("rejected") || err.contains("escapes") || err.contains("workspace guard"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn resolve_user_path_contain_rejects_absolute_path_outside_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let abs = dir
            .path()
            .parent()
            .unwrap()
            .join("outside-meta.txt")
            .to_string_lossy()
            .into_owned();
        let g = GlobalFlags {
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            contain: true,
            ..GlobalFlags::default()
        };
        let err = g.resolve_user_path(&abs).unwrap_err().to_string();
        // AllowIfContained: outside absolute fails as escape/reject, not blanket "no absolutes".
        assert!(
            err.contains("escapes")
                || err.contains("rejected")
                || err.contains("workspace guard")
                || err.contains("absolute"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn resolve_user_path_contain_allows_absolute_path_inside_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let abs = dir.path().join("ops.txt");
        let g = GlobalFlags {
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            contain: true,
            ..GlobalFlags::default()
        };
        let resolved = g.resolve_user_path(&abs).unwrap();
        assert_eq!(resolved, abs);
    }

    #[test]
    fn resolve_user_path_contain_allows_relative_inside() {
        let dir = tempfile::tempdir().unwrap();
        let g = GlobalFlags {
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            contain: true,
            ..GlobalFlags::default()
        };
        let resolved = g.resolve_user_path("ops.txt").unwrap();
        assert_eq!(resolved, dir.path().join("ops.txt"));
    }

    #[test]
    fn resolve_user_path_empty_is_invalid_input() {
        let g = GlobalFlags::default();
        let err = g.resolve_user_path("").unwrap_err();
        assert!(
            crate::exit::is_invalid_input(&err),
            "empty path should be InvalidInputError: {err}"
        );
        assert!(err.to_string().contains("path must not be empty"));
    }

    #[test]
    fn read_files_from_contain_rejects_list_outside_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().parent().unwrap().join(format!(
            "patchloom-files-from-outside-{}.txt",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::write(&outside, "in.txt\n").unwrap();
        let flags = GlobalFlags {
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            contain: true,
            files_from: Some(outside.to_string_lossy().into_owned()),
            ..GlobalFlags::test_default()
        };
        let err = flags.read_files_from().unwrap_err().to_string();
        let _ = std::fs::remove_file(&outside);
        assert!(
            err.contains("escapes")
                || err.contains("rejected")
                || err.contains("workspace guard")
                || err.contains("absolute"),
            "must not open outside list under --contain: {err}"
        );
    }

    #[test]
    fn collect_files_from_line_results_propagates_io_error() {
        let lines = vec![
            Ok("a.txt".into()),
            Err(std::io::Error::other("simulated stdin failure")),
            Ok("b.txt".into()),
        ];
        let err = collect_files_from_line_results(lines)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("failed to read --files-from from stdin"),
            "unexpected: {err}"
        );
        assert!(err.contains("simulated stdin failure"), "unexpected: {err}");
    }

    #[test]
    fn collect_files_from_line_results_trims_and_strips_bom() {
        let lines = vec![
            Ok("\u{FEFF}  a.txt  ".into()),
            Ok(String::new()),
            Ok("  b.txt\t".into()),
        ];
        let got = collect_files_from_line_results(lines).unwrap();
        assert_eq!(got, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn read_files_from_trims_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let list = dir.path().join("files.txt");
        std::fs::write(&list, "  src/main.rs  \n  lib.rs\t\n").unwrap();
        let flags = GlobalFlags {
            files_from: Some(list.to_str().unwrap().to_string()),
            ..GlobalFlags::test_default()
        };
        let result = flags.read_files_from().unwrap().unwrap();
        assert_eq!(result, vec!["src/main.rs", "lib.rs"]);
    }

    #[test]
    fn read_files_from_strips_bom() {
        let dir = tempfile::tempdir().unwrap();
        let list = dir.path().join("files.txt");
        std::fs::write(&list, "\u{FEFF}src/main.rs\nlib.rs\n").unwrap();
        let flags = GlobalFlags {
            files_from: Some(list.to_str().unwrap().to_string()),
            ..GlobalFlags::test_default()
        };
        let result = flags.read_files_from().unwrap().unwrap();
        assert_eq!(result, vec!["src/main.rs", "lib.rs"]);
    }

    #[test]
    fn read_files_from_whitespace_only_lines_filtered() {
        let dir = tempfile::tempdir().unwrap();
        let list = dir.path().join("files.txt");
        std::fs::write(&list, "src/main.rs\n   \nlib.rs\n").unwrap();
        let flags = GlobalFlags {
            files_from: Some(list.to_str().unwrap().to_string()),
            ..GlobalFlags::test_default()
        };
        let result = flags.read_files_from().unwrap().unwrap();
        assert_eq!(result, vec!["src/main.rs", "lib.rs"]);
    }

    #[test]
    fn read_files_from_resolves_relative_list_under_cwd_flag() {
        let dir = tempfile::tempdir().unwrap();
        let list = dir.path().join("files.txt");
        std::fs::write(&list, "a.rs\nb.rs\n").unwrap();
        let flags = GlobalFlags {
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            // Relative list name; must open under --cwd, not process cwd.
            files_from: Some("files.txt".into()),
            ..GlobalFlags::test_default()
        };
        let result = flags.read_files_from().unwrap().unwrap();
        assert_eq!(result, vec!["a.rs", "b.rs"]);
    }

    #[test]
    fn path_scope_description_prefers_files_from_over_empty_paths() {
        let stdin = GlobalFlags {
            files_from: Some("-".into()),
            ..GlobalFlags::test_default()
        };
        assert_eq!(stdin.path_scope_description(&[]), "--files-from -");
        assert_eq!(
            stdin.path_scope_description(&["src/".into()]),
            "--files-from -"
        );

        let list = GlobalFlags {
            files_from: Some("paths.txt".into()),
            ..GlobalFlags::test_default()
        };
        assert_eq!(list.path_scope_description(&[]), "--files-from paths.txt");

        let walk = GlobalFlags::test_default();
        assert_eq!(walk.path_scope_description(&[]), ".");
        assert_eq!(
            walk.path_scope_description(&["a".into(), "b".into()]),
            "a, b"
        );
    }
}
