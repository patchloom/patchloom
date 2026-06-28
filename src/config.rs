//! Project configuration file support (.patchloom.toml).
//!
//! Searches from the working directory upward for a `.patchloom.toml` file and
//! parses it into [`ProjectConfig`]. CLI flags override config file values.

use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::write::WritePolicyOverride;

/// Project-level configuration loaded from `.patchloom.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct ProjectConfig {
    pub write_policy: WritePolicyOverride,
    pub exclude: Exclude,
    pub output: Output,
    pub tx: TxConfig,
    pub defaults: Defaults,
    pub format: FormatConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct Defaults {
    /// Default to --apply mode. When true, write commands apply changes without needing --apply flag.
    pub apply: Option<bool>,
    /// Default format command (e.g., "cargo fmt"). Applied after --apply unless overridden by CLI --format.
    pub format: Option<String>,
}

/// Format configuration: per-extension formatters and auto-format toggle.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct FormatConfig {
    /// When true, run formatters automatically after --apply without needing --format.
    pub auto: Option<bool>,
    /// Single catch-all formatter command (runs on the entire cwd).
    pub command: Option<String>,
    /// Per-extension formatter commands. Keys are file extensions (without dot),
    /// values are shell commands. The file path is appended as the last argument.
    #[serde(default)]
    pub by_extension: std::collections::HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct TxConfig {
    pub strict: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct Exclude {
    #[serde(default)]
    pub globs: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct Output {
    pub color: Option<String>,
}

/// Search for `.patchloom.toml` starting from `start` and walking up to the
/// filesystem root. Returns the parsed config and its directory, or `None` if
/// no config file is found.
pub fn find_and_load(start: &Path) -> Option<(ProjectConfig, PathBuf)> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join(".patchloom.toml");
        if candidate.is_file() {
            let content = match std::fs::read_to_string(&candidate) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("warning: could not read {}: {}", candidate.display(), e);
                    return None;
                }
            };
            match toml_edit::de::from_str::<ProjectConfig>(&content) {
                Ok(config) => return Some((config, dir)),
                Err(e) => {
                    eprintln!("warning: malformed {}: {}", candidate.display(), e);
                    return None;
                }
            }
        }
        // Stop at repo root to avoid loading configs from parent directories.
        if dir.join(".git").exists() {
            return None;
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Merge a [`ProjectConfig`] into [`GlobalFlags`](crate::cli::global::GlobalFlags), applying config values only
/// where the CLI did not explicitly set them.
///
/// Apply config values as defaults into GlobalFlags (CLI flags still win where set).
/// Available for library tx execution as well as CLI.
pub fn apply_config(global: &mut crate::cli::global::GlobalFlags, config: &ProjectConfig) {
    // Write policy: config provides defaults, CLI flags win.
    if !global.ensure_final_newline && config.write_policy.ensure_final_newline == Some(true) {
        global.ensure_final_newline = true;
    }
    if global.normalize_eol.is_none() {
        match config.write_policy.normalize_eol.as_deref() {
            Some("lf") => global.normalize_eol = Some(crate::cli::global::EolMode::Lf),
            Some("crlf") => global.normalize_eol = Some(crate::cli::global::EolMode::Crlf),
            Some("cr") => global.normalize_eol = Some(crate::cli::global::EolMode::Cr),
            Some(invalid) => {
                eprintln!("{}", invalid_normalize_eol_warning(invalid));
            }
            None => {}
        }
    }
    if !global.trim_trailing_whitespace
        && config.write_policy.trim_trailing_whitespace == Some(true)
    {
        global.trim_trailing_whitespace = true;
    }
    if !global.collapse_blanks && config.write_policy.collapse_blanks == Some(true) {
        global.collapse_blanks = true;
    }
    if !global.respect_editorconfig && config.write_policy.respect_editorconfig == Some(true) {
        global.respect_editorconfig = true;
    }

    // Defaults: config provides defaults, CLI flags win.
    // Only apply config's defaults.apply if no explicit mode flag was set.
    // If the user passed --check or --diff, they explicitly want a non-apply mode
    // and the config should not override that.
    if !global.apply
        && !global.check
        && !global.diff
        && !global.confirm
        && config.defaults.apply == Some(true)
    {
        global.apply = true;
    }
    if global.format.is_none() {
        // CLI --format wins, then defaults.format, then format.command (with auto=true)
        if let Some(ref fmt) = config.defaults.format {
            global.format = Some(fmt.clone());
        } else if config.format.auto == Some(true)
            && let Some(ref cmd) = config.format.command
        {
            global.format = Some(cmd.clone());
        }
    }

    // Exclude globs: prepend config globs before user excludes.
    if !config.exclude.globs.is_empty() {
        let mut merged = config.exclude.globs.clone();
        merged.append(&mut global.exclude);
        global.exclude = merged;
    }

    // Color: config provides default, CLI wins.
    if matches!(global.color, crate::cli::global::ColorMode::Auto)
        && let Some(ref color) = config.output.color
    {
        global.color = match color.as_str() {
            "always" => crate::cli::global::ColorMode::Always,
            "never" => crate::cli::global::ColorMode::Never,
            "auto" => crate::cli::global::ColorMode::Auto,
            invalid => {
                eprintln!("{}", invalid_output_color_warning(invalid));
                crate::cli::global::ColorMode::Auto
            }
        };
    }
}

fn invalid_normalize_eol_warning(invalid: &str) -> String {
    format!(
        "warning: invalid write_policy.normalize_eol value {invalid:?}; expected \"lf\", \"crlf\", or \"cr\""
    )
}

fn invalid_output_color_warning(invalid: &str) -> String {
    format!(
        "warning: invalid output.color value {invalid:?}; expected \"always\" or \"never\". Using \"auto\"."
    )
}

/// A cached project configuration that can be loaded once and reused across
/// multiple operations. This avoids re-reading `.patchloom.toml` from disk
/// on every API call.
///
/// # Thread safety
///
/// `CachedConfig` is `Send + Sync` and can be shared across threads via
/// `Arc<CachedConfig>`.
///
/// # Example
///
/// ```rust,no_run
/// use patchloom::config::CachedConfig;
/// use std::path::Path;
///
/// let config = CachedConfig::load(Path::new("/my/project"));
/// // Use config.project_config() in multiple API calls...
/// ```
#[derive(Debug)]
pub struct CachedConfig {
    config: ProjectConfig,
    config_dir: Option<PathBuf>,
}

impl CachedConfig {
    /// Load configuration from the given directory, searching upward for
    /// `.patchloom.toml`. Returns a `CachedConfig` with defaults if no
    /// config file is found.
    pub fn load(start: &Path) -> Self {
        match find_and_load(start) {
            Some((config, dir)) => Self {
                config,
                config_dir: Some(dir),
            },
            None => Self {
                config: ProjectConfig::default(),
                config_dir: None,
            },
        }
    }

    /// Access the loaded project configuration.
    pub fn project_config(&self) -> &ProjectConfig {
        &self.config
    }

    /// The directory containing the `.patchloom.toml` file, if one was found.
    pub fn config_dir(&self) -> Option<&Path> {
        self.config_dir.as_deref()
    }
}

// Static assertion: CachedConfig must be Send + Sync for cross-thread sharing.
const _: () = {
    fn _assert<T: Send + Sync>() {}
    let _ = _assert::<CachedConfig>;
};

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn find_and_load_from_dir() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join(".patchloom.toml"),
            r#"
[write_policy]
ensure_final_newline = true
normalize_eol = "lf"
collapse_blanks = true

[exclude]
globs = ["target/**"]

[output]
color = "always"
"#,
        )
        .unwrap();

        let (config, found_dir) = find_and_load(dir.path()).unwrap();
        assert_eq!(found_dir, dir.path());
        assert_eq!(config.write_policy.ensure_final_newline, Some(true));
        assert_eq!(config.write_policy.normalize_eol.as_deref(), Some("lf"));
        assert_eq!(config.write_policy.collapse_blanks, Some(true));
        assert_eq!(config.exclude.globs, vec!["target/**"]);
        assert_eq!(config.output.color.as_deref(), Some("always"));
    }

    #[test]
    fn find_and_load_walks_up() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join(".patchloom.toml"),
            "[write_policy]\nensure_final_newline = true\n",
        )
        .unwrap();

        let sub = dir.path().join("sub/deep");
        std::fs::create_dir_all(&sub).unwrap();

        let (config, found_dir) = find_and_load(&sub).unwrap();
        assert_eq!(found_dir, dir.path());
        assert_eq!(config.write_policy.ensure_final_newline, Some(true));
    }

    #[test]
    fn find_and_load_tx_strict_override() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(".patchloom.toml"), "[tx]\nstrict = false\n").unwrap();

        let (config, _) = find_and_load(dir.path()).unwrap();
        assert_eq!(config.tx.strict, Some(false));
    }

    #[test]
    fn find_and_load_returns_none_when_missing() {
        let dir = TempDir::new().unwrap();
        assert!(find_and_load(dir.path()).is_none());
    }

    #[test]
    fn find_and_load_stops_at_git_boundary() {
        let dir = TempDir::new().unwrap();
        // Place a config in the parent directory.
        std::fs::write(
            dir.path().join(".patchloom.toml"),
            "[write_policy]\nensure_final_newline = true\n",
        )
        .unwrap();

        // Create a child directory that is a separate git repo root.
        let child = dir.path().join("child");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::create_dir_all(child.join(".git")).unwrap();

        // Searching from child should NOT find the parent's config.
        assert!(find_and_load(&child).is_none());
    }

    #[test]
    #[cfg(feature = "cli")]
    fn apply_config_sets_defaults() {
        let config = ProjectConfig {
            write_policy: WritePolicyOverride {
                ensure_final_newline: Some(true),
                normalize_eol: Some("lf".into()),
                trim_trailing_whitespace: Some(true),
                collapse_blanks: Some(true),
                respect_editorconfig: Some(true),
            },
            exclude: Exclude {
                globs: vec!["target/**".into()],
            },
            output: Output {
                color: Some("always".into()),
            },
            tx: TxConfig::default(),
            defaults: Defaults::default(),
            format: FormatConfig::default(),
        };
        let mut global = crate::cli::global::GlobalFlags::default();
        apply_config(&mut global, &config);

        assert!(global.ensure_final_newline);
        assert!(matches!(
            global.normalize_eol,
            Some(crate::cli::global::EolMode::Lf)
        ));
        assert!(global.trim_trailing_whitespace);
        assert!(global.collapse_blanks);
        assert!(global.respect_editorconfig);
        assert_eq!(global.exclude, vec!["target/**"]);
        assert!(
            global.glob.is_empty(),
            "exclude globs must not leak into include globs"
        );
        assert!(matches!(
            global.color,
            crate::cli::global::ColorMode::Always
        ));
    }

    #[test]
    #[cfg(feature = "cli")]
    fn apply_config_cli_flags_win() {
        let config = ProjectConfig {
            write_policy: WritePolicyOverride {
                ensure_final_newline: Some(true),
                ..WritePolicyOverride::default()
            },
            exclude: Exclude {
                globs: vec!["config_glob".into()],
            },
            ..ProjectConfig::default()
        };
        let mut global = crate::cli::global::GlobalFlags {
            ensure_final_newline: true,
            glob: vec!["user_glob".into()],
            color: crate::cli::global::ColorMode::Never,
            ..crate::cli::global::GlobalFlags::default()
        };
        apply_config(&mut global, &config);

        // CLI flag already set, config doesn't override
        assert!(global.ensure_final_newline);
        // Config exclude globs go to exclude, not include globs
        assert_eq!(global.exclude, vec!["config_glob"]);
        // User include globs remain unchanged
        assert_eq!(global.glob, vec!["user_glob"]);
        // CLI color wins
        assert!(matches!(global.color, crate::cli::global::ColorMode::Never));
    }

    #[test]
    fn find_and_load_rejects_unknown_keys() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join(".patchloom.toml"),
            "[write_policy]\nensur_final_newline = true\n",
        )
        .unwrap();

        // Typo in key name is rejected, not silently ignored.
        assert!(find_and_load(dir.path()).is_none());
    }

    #[test]
    fn find_and_load_returns_none_on_malformed_toml() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join(".patchloom.toml"),
            "this is not valid { toml [",
        )
        .unwrap();

        // Malformed TOML returns None but prints a warning to stderr.
        assert!(find_and_load(dir.path()).is_none());
    }

    #[test]
    #[cfg(feature = "cli")]
    fn apply_config_unknown_eol_value_ignored() {
        let config = ProjectConfig {
            write_policy: WritePolicyOverride {
                normalize_eol: Some("CRLF".into()), // uppercase: not recognized
                ..WritePolicyOverride::default()
            },
            ..ProjectConfig::default()
        };
        let mut global = crate::cli::global::GlobalFlags::default();
        apply_config(&mut global, &config);

        assert!(global.normalize_eol.is_none());
        let warning = invalid_normalize_eol_warning("CRLF");
        assert!(warning.contains("CRLF"));
        assert!(warning.contains("\"lf\""));
        assert!(warning.contains("\"crlf\""));
        assert!(warning.contains("\"cr\""));
    }

    #[test]
    #[cfg(feature = "cli")]
    fn apply_config_crlf_eol() {
        let config = ProjectConfig {
            write_policy: WritePolicyOverride {
                normalize_eol: Some("crlf".into()),
                ..WritePolicyOverride::default()
            },
            ..ProjectConfig::default()
        };
        let mut global = crate::cli::global::GlobalFlags::default();
        apply_config(&mut global, &config);

        assert!(matches!(
            global.normalize_eol,
            Some(crate::cli::global::EolMode::Crlf)
        ));
    }

    #[test]
    #[cfg(feature = "cli")]
    // Regression: color = "auto" in config produced a spurious warning because
    // the match only handled "always" and "never", falling through to the
    // invalid-value catch-all for "auto".
    fn apply_config_color_auto_no_warning() {
        let config = ProjectConfig {
            output: Output {
                color: Some("auto".into()),
            },
            ..ProjectConfig::default()
        };
        let mut global = crate::cli::global::GlobalFlags::default();
        apply_config(&mut global, &config);
        assert!(matches!(global.color, crate::cli::global::ColorMode::Auto));
    }

    #[test]
    fn apply_config_unknown_color_value_stays_auto() {
        let config = ProjectConfig {
            output: Output {
                color: Some("yes".into()), // not "always" or "never"
            },
            ..ProjectConfig::default()
        };
        let mut global = crate::cli::global::GlobalFlags::default();
        apply_config(&mut global, &config);

        assert!(matches!(global.color, crate::cli::global::ColorMode::Auto));
        let warning = invalid_output_color_warning("yes");
        assert!(warning.contains("yes"));
        assert!(warning.contains("always"));
        assert!(warning.contains("never"));
    }

    #[test]
    fn cached_config_load_with_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join(".patchloom.toml"),
            "[write_policy]\nensure_final_newline = true\n",
        )
        .unwrap();

        let cached = CachedConfig::load(dir.path());
        assert_eq!(
            cached.project_config().write_policy.ensure_final_newline,
            Some(true)
        );
        assert_eq!(cached.config_dir(), Some(dir.path()));
    }

    #[test]
    fn cached_config_load_defaults_when_missing() {
        let dir = TempDir::new().unwrap();

        let cached = CachedConfig::load(dir.path());
        assert_eq!(
            cached.project_config().write_policy.ensure_final_newline,
            None
        );
        assert!(cached.config_dir().is_none());
    }

    #[test]
    #[cfg(feature = "cli")]
    fn apply_config_respect_editorconfig_from_config() {
        let config = ProjectConfig {
            write_policy: WritePolicyOverride {
                respect_editorconfig: Some(true),
                ..WritePolicyOverride::default()
            },
            ..ProjectConfig::default()
        };
        let mut global = crate::cli::global::GlobalFlags::default();
        assert!(!global.respect_editorconfig);
        apply_config(&mut global, &config);
        assert!(global.respect_editorconfig);
    }

    #[test]
    #[cfg(feature = "cli")]
    fn apply_config_apply_default_from_config() {
        let config = ProjectConfig {
            defaults: Defaults {
                apply: Some(true),
                ..Defaults::default()
            },
            ..ProjectConfig::default()
        };
        let mut global = crate::cli::global::GlobalFlags::default();
        assert!(!global.apply);
        apply_config(&mut global, &config);
        assert!(global.apply);
    }

    // Regression: defaults.apply must not override explicit --check or --diff.
    #[test]
    fn apply_config_apply_default_respects_check_flag() {
        let config = ProjectConfig {
            defaults: Defaults {
                apply: Some(true),
                ..Defaults::default()
            },
            ..ProjectConfig::default()
        };
        let mut global = crate::cli::global::GlobalFlags {
            check: true, // user explicitly passed --check
            ..crate::cli::global::GlobalFlags::default()
        };
        apply_config(&mut global, &config);
        assert!(
            !global.apply,
            "defaults.apply should not override explicit --check"
        );
    }

    #[test]
    fn apply_config_apply_default_respects_diff_flag() {
        let config = ProjectConfig {
            defaults: Defaults {
                apply: Some(true),
                ..Defaults::default()
            },
            ..ProjectConfig::default()
        };
        let mut global = crate::cli::global::GlobalFlags {
            diff: true, // user explicitly passed --diff
            ..crate::cli::global::GlobalFlags::default()
        };
        apply_config(&mut global, &config);
        assert!(
            !global.apply,
            "defaults.apply should not override explicit --diff"
        );
    }

    #[test]
    #[cfg(feature = "cli")]
    fn apply_config_format_default_from_config() {
        let config = ProjectConfig {
            defaults: Defaults {
                format: Some("cargo fmt --all".into()),
                ..Defaults::default()
            },
            ..ProjectConfig::default()
        };
        let mut global = crate::cli::global::GlobalFlags::default();
        assert!(global.format.is_none());
        apply_config(&mut global, &config);
        assert_eq!(global.format.as_deref(), Some("cargo fmt --all"));
    }

    #[test]
    fn find_and_load_with_defaults() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join(".patchloom.toml"),
            r#"
[defaults]
apply = true
format = "cargo fmt"

[write_policy]
respect_editorconfig = true
"#,
        )
        .unwrap();

        let (config, found_dir) = find_and_load(dir.path()).unwrap();
        assert_eq!(found_dir, dir.path());
        assert_eq!(config.defaults.apply, Some(true));
        assert_eq!(config.defaults.format.as_deref(), Some("cargo fmt"));
        assert_eq!(config.write_policy.respect_editorconfig, Some(true));
    }

    #[test]
    fn find_and_load_format_config() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join(".patchloom.toml"),
            r#"
[format]
auto = true
command = "treefmt"

[format.by_extension]
rs = "rustfmt"
go = "gofmt -w"
ts = "prettier --write"
"#,
        )
        .unwrap();

        let (config, _) = find_and_load(dir.path()).unwrap();
        assert_eq!(config.format.auto, Some(true));
        assert_eq!(config.format.command.as_deref(), Some("treefmt"));
        assert_eq!(config.format.by_extension.len(), 3);
        assert_eq!(config.format.by_extension.get("rs").unwrap(), "rustfmt");
        assert_eq!(config.format.by_extension.get("go").unwrap(), "gofmt -w");
        assert_eq!(
            config.format.by_extension.get("ts").unwrap(),
            "prettier --write"
        );
    }

    #[test]
    fn find_and_load_format_auto_without_command() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join(".patchloom.toml"),
            r#"
[format]
auto = true

[format.by_extension]
rs = "rustfmt"
"#,
        )
        .unwrap();

        let (config, _) = find_and_load(dir.path()).unwrap();
        assert_eq!(config.format.auto, Some(true));
        assert!(config.format.command.is_none());
        assert_eq!(config.format.by_extension.get("rs").unwrap(), "rustfmt");
    }

    #[test]
    #[cfg(feature = "cli")]
    fn apply_config_format_auto_command_sets_global_format() {
        let config = ProjectConfig {
            format: FormatConfig {
                auto: Some(true),
                command: Some("treefmt".into()),
                ..FormatConfig::default()
            },
            ..ProjectConfig::default()
        };
        let mut global = crate::cli::global::GlobalFlags::default();
        assert!(global.format.is_none());
        apply_config(&mut global, &config);
        assert_eq!(global.format.as_deref(), Some("treefmt"));
    }

    #[test]
    #[cfg(feature = "cli")]
    fn apply_config_defaults_format_wins_over_format_command() {
        let config = ProjectConfig {
            defaults: Defaults {
                format: Some("cargo fmt --all".into()),
                ..Defaults::default()
            },
            format: FormatConfig {
                auto: Some(true),
                command: Some("treefmt".into()),
                ..FormatConfig::default()
            },
            ..ProjectConfig::default()
        };
        let mut global = crate::cli::global::GlobalFlags::default();
        apply_config(&mut global, &config);
        // defaults.format takes priority over format.command
        assert_eq!(global.format.as_deref(), Some("cargo fmt --all"));
    }
}
