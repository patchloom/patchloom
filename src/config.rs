//! Project configuration file support (.patchloom.toml).
//!
//! Searches from the working directory upward for a `.patchloom.toml` file and
//! parses it into [`ProjectConfig`]. CLI flags override config file values.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Project-level configuration loaded from `.patchloom.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectConfig {
    pub write_policy: WritePolicy,
    pub exclude: Exclude,
    pub output: Output,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WritePolicy {
    pub ensure_final_newline: Option<bool>,
    pub normalize_eol: Option<String>,
    pub trim_trailing_whitespace: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Exclude {
    #[serde(default)]
    pub globs: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
        if !dir.pop() {
            return None;
        }
    }
}

/// Merge a [`ProjectConfig`] into [`GlobalFlags`], applying config values only
/// where the CLI did not explicitly set them.
pub fn apply_config(global: &mut crate::cli::global::GlobalFlags, config: &ProjectConfig) {
    // Write policy: config provides defaults, CLI flags win.
    if !global.ensure_final_newline && config.write_policy.ensure_final_newline == Some(true) {
        global.ensure_final_newline = true;
    }
    if global.normalize_eol.is_none() {
        global.normalize_eol =
            config
                .write_policy
                .normalize_eol
                .as_deref()
                .and_then(|eol| match eol {
                    "lf" => Some(crate::cli::global::EolMode::Lf),
                    "crlf" => Some(crate::cli::global::EolMode::Crlf),
                    _ => None,
                });
    }
    if !global.trim_trailing_whitespace
        && config.write_policy.trim_trailing_whitespace == Some(true)
    {
        global.trim_trailing_whitespace = true;
    }

    // Exclude globs: prepend config globs before user globs.
    if !config.exclude.globs.is_empty() {
        let mut merged = config.exclude.globs.clone();
        merged.append(&mut global.glob);
        global.glob = merged;
    }

    // Color: config provides default, CLI wins.
    if matches!(global.color, crate::cli::global::ColorMode::Auto)
        && let Some(ref color) = config.output.color
    {
        global.color = match color.as_str() {
            "always" => crate::cli::global::ColorMode::Always,
            "never" => crate::cli::global::ColorMode::Never,
            _ => crate::cli::global::ColorMode::Auto,
        };
    }
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
    fn find_and_load_returns_none_when_missing() {
        let dir = TempDir::new().unwrap();
        assert!(find_and_load(dir.path()).is_none());
    }

    #[test]
    fn apply_config_sets_defaults() {
        let config = ProjectConfig {
            write_policy: WritePolicy {
                ensure_final_newline: Some(true),
                normalize_eol: Some("lf".into()),
                trim_trailing_whitespace: Some(true),
            },
            exclude: Exclude {
                globs: vec!["target/**".into()],
            },
            output: Output {
                color: Some("always".into()),
            },
        };
        let mut global = crate::cli::global::GlobalFlags::default();
        apply_config(&mut global, &config);

        assert!(global.ensure_final_newline);
        assert!(matches!(
            global.normalize_eol,
            Some(crate::cli::global::EolMode::Lf)
        ));
        assert!(global.trim_trailing_whitespace);
        assert_eq!(global.glob, vec!["target/**"]);
        assert!(matches!(
            global.color,
            crate::cli::global::ColorMode::Always
        ));
    }

    #[test]
    fn apply_config_cli_flags_win() {
        let config = ProjectConfig {
            write_policy: WritePolicy {
                ensure_final_newline: Some(true),
                ..WritePolicy::default()
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
        // Config globs prepended before user globs
        assert_eq!(global.glob, vec!["config_glob", "user_glob"]);
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
    fn apply_config_unknown_eol_value_ignored() {
        let config = ProjectConfig {
            write_policy: WritePolicy {
                normalize_eol: Some("CRLF".into()), // uppercase: not recognized
                ..WritePolicy::default()
            },
            ..ProjectConfig::default()
        };
        let mut global = crate::cli::global::GlobalFlags::default();
        apply_config(&mut global, &config);

        // Unrecognized normalize_eol value is silently ignored (stays None).
        assert!(global.normalize_eol.is_none());
    }

    #[test]
    fn apply_config_crlf_eol() {
        let config = ProjectConfig {
            write_policy: WritePolicy {
                normalize_eol: Some("crlf".into()),
                ..WritePolicy::default()
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
    fn apply_config_unknown_color_value_stays_auto() {
        let config = ProjectConfig {
            output: Output {
                color: Some("yes".into()), // not "always" or "never"
            },
            ..ProjectConfig::default()
        };
        let mut global = crate::cli::global::GlobalFlags::default();
        apply_config(&mut global, &config);

        // Unrecognized color value falls through to Auto.
        assert!(matches!(global.color, crate::cli::global::ColorMode::Auto));
    }
}
