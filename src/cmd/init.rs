use crate::cli::global::GlobalFlags;
use crate::cmd::AGENT_RULES_GENERATED_MARKER;
use crate::exit;
use anyhow::Context;
use clap::Args;
use std::path::{Path, PathBuf};

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom init
  patchloom init --yes")]
pub struct InitArgs {
    /// Skip confirmation prompts.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

/// Candidate files where agent instructions may already exist.
const AGENT_FILES: &[&str] = &[
    "AGENTS.md",
    "Agents.md",
    "AGENT.md",
    "Claude.md",
    "CLAUDE.md",
    "PATCHLOOM.md",
];

pub fn run(args: InitArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    let auto_yes = args.yes;
    let quiet = global.quiet;

    // Helper: print to stderr unless --quiet.
    macro_rules! status {
        ($($arg:tt)*) => {
            if !quiet { eprintln!($($arg)*); }
        };
    }

    // 1. Generate and write agent rules.
    let rules = super::generate_agent_rules(&super::AgentRulesArgs {
        mode: super::AgentMode::All,
        platform: super::AgentPlatform::All,
    });

    let target = find_agent_file(&cwd);
    let (target_path, action) = match target {
        Some(existing) => (existing, "append"),
        None => (cwd.join("AGENTS.md"), "create"),
    };
    let rel_target = target_path
        .strip_prefix(&cwd)
        .unwrap_or(&target_path)
        .display();

    if action == "append" {
        // Check if patchloom rules are already present.
        let content = std::fs::read_to_string(&target_path)
            .with_context(|| format!("reading existing {}", target_path.display()))?;
        if content.contains(AGENT_RULES_GENERATED_MARKER) {
            status!("{rel_target} already contains patchloom rules, skipping.");
        } else if auto_yes || confirm(&format!("Append patchloom rules to {rel_target}?")) {
            let mut content = content;
            if !content.ends_with('\n') {
                content.push('\n');
            }
            content.push('\n');
            content.push_str(&rules);
            std::fs::write(&target_path, content)
                .with_context(|| format!("writing {}", target_path.display()))?;
            status!("appended patchloom rules to {rel_target}");
        } else {
            status!("skipped {rel_target}");
        }
    } else if auto_yes || confirm(&format!("Create {rel_target}?")) {
        std::fs::write(&target_path, &rules)
            .with_context(|| format!("writing {}", target_path.display()))?;
        status!("created {rel_target}");
    } else {
        status!("skipped {rel_target}");
    }

    // 2. Shell completions: auto-install or hint.
    if let Some(shell) = detect_shell() {
        if let Some(target) = completion_install_path(&shell) {
            let parent_ready = target.parent().map_or(Ok(()), |parent| {
                if parent.exists() {
                    Ok(())
                } else {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("creating {}", parent.display()))
                }
            });
            if let Err(e) = parent_ready {
                status!("failed to prepare completion directory: {e}");
                let cmd = completion_command(&shell);
                status!("\nshell completions ({shell}):");
                status!("  {cmd}");
            } else if auto_yes
                || confirm(&format!(
                    "Install {} completions to {}?",
                    shell,
                    target.display()
                ))
            {
                match generate_completions(&shell, &target) {
                    Ok(()) => status!("installed {shell} completions to {}", target.display()),
                    Err(e) => status!("failed to install completions: {e}"),
                }
            } else {
                let cmd = completion_command(&shell);
                status!("\nshell completions ({shell}):");
                status!("  {cmd}");
            }
        } else {
            let cmd = completion_command(&shell);
            status!("\nshell completions ({shell}):");
            status!("  {cmd}");
        }
    } else if !quiet {
        eprintln!("\nshell completions:");
        eprintln!("  patchloom completions <bash|zsh|fish|elvish>");
    }

    if !quiet {
        // 3. MCP setup hint.
        eprintln!();
        if cfg!(feature = "mcp") {
            let mcp_json_hint = r#"    { "servers": { "patchloom": { "command": "patchloom", "args": ["mcp-server"] } } }"#;
            eprintln!("MCP server is available. Add to your agent's config:");
            if cwd.join(".grok").is_dir() || home_file_exists(".grok/config.toml") {
                eprintln!("  Grok: add to ~/.grok/config.toml:");
                eprintln!("    [mcp_servers.patchloom]");
                eprintln!("    command = \"patchloom\"");
                eprintln!("    args = [\"mcp-server\"]");
            }
            if cwd.join(".vscode").is_dir() {
                eprintln!("  VS Code: create .vscode/mcp.json:");
                eprintln!("{mcp_json_hint}");
            }
            if cwd.join(".cursor").is_dir() {
                eprintln!("  Cursor: create .cursor/mcp.json:");
                eprintln!("{mcp_json_hint}");
            }
        } else {
            eprintln!(
                "MCP server not available (this binary was built with --no-default-features)."
            );
        }

        eprintln!();
        eprintln!("setup complete.");
    }
    Ok(exit::SUCCESS)
}

fn find_agent_file(cwd: &Path) -> Option<std::path::PathBuf> {
    let entries: Vec<_> = std::fs::read_dir(cwd)
        .ok()?
        .filter_map(Result::ok)
        .collect();
    for name in AGENT_FILES {
        for entry in &entries {
            if entry.file_name() == std::ffi::OsStr::new(name) {
                return Some(entry.path());
            }
        }
    }
    None
}

fn detect_shell() -> Option<String> {
    std::env::var("SHELL").ok().and_then(|s| {
        let name = Path::new(&s).file_name()?.to_str()?.to_string();
        match name.as_str() {
            "bash" | "zsh" | "fish" | "elvish" => Some(name),
            _ => None,
        }
    })
}

fn completion_command(shell: &str) -> String {
    match shell {
        "bash" => "patchloom completions bash > /etc/bash_completion.d/patchloom".into(),
        "zsh" => "patchloom completions zsh > ~/.zfunc/_patchloom".into(),
        "fish" => "patchloom completions fish > ~/.config/fish/completions/patchloom.fish".into(),
        "elvish" => "patchloom completions elvish > ~/.config/elvish/rc.elv".into(),
        _ => format!("patchloom completions {shell}"),
    }
}

fn completion_install_path(shell: &str) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let home = Path::new(&home);
    match shell {
        "bash" => {
            let xdg = home.join(".local/share/bash-completion/completions");
            if xdg.is_dir() {
                return Some(xdg.join("patchloom"));
            }
            // System path as fallback (may need root).
            let sys = Path::new("/etc/bash_completion.d/patchloom");
            Some(sys.to_path_buf())
        }
        "zsh" => Some(home.join(".zfunc/_patchloom")),
        "fish" => Some(home.join(".config/fish/completions/patchloom.fish")),
        _ => None,
    }
}

fn generate_completions(shell: &str, target: &Path) -> anyhow::Result<()> {
    use crate::cli::Cli;
    let clap_shell = match shell {
        "bash" => clap_complete::Shell::Bash,
        "zsh" => clap_complete::Shell::Zsh,
        "fish" => clap_complete::Shell::Fish,
        "elvish" => clap_complete::Shell::Elvish,
        _ => anyhow::bail!("unsupported shell: {shell}"),
    };
    let mut cmd = <Cli as clap::CommandFactory>::command();
    let mut buf = Vec::new();
    clap_complete::generate(clap_shell, &mut cmd, "patchloom", &mut buf);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    std::fs::write(target, buf)
        .with_context(|| format!("writing completions to {}", target.display()))?;
    Ok(())
}

fn home_file_exists(rel: &str) -> bool {
    if let Some(home) = std::env::var_os("HOME") {
        return Path::new(&home).join(rel).exists();
    }
    false
}

fn confirm(prompt: &str) -> bool {
    crate::cli::global::confirm_prompt(prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_shell_from_env() {
        // Just ensure the function doesn't panic.
        let _ = detect_shell();
    }

    #[test]
    fn completion_command_known_shells() {
        assert!(completion_command("bash").contains("bash_completion"));
        assert!(completion_command("zsh").contains("_patchloom"));
        assert!(completion_command("fish").contains("completions/patchloom.fish"));
    }

    #[test]
    fn find_agent_file_none_in_empty_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(find_agent_file(dir.path()).is_none());
    }

    #[test]
    fn find_agent_file_finds_agents_md() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "# Rules\n").unwrap();
        let found = find_agent_file(dir.path()).expect("should find AGENTS.md");
        assert!(found.ends_with("AGENTS.md"));
    }

    #[test]
    fn find_agent_file_preserves_claude_md_case() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("Claude.md"), "# Rules\n").unwrap();
        let found = find_agent_file(dir.path()).expect("should find Claude.md");
        assert!(found.ends_with("Claude.md"));
    }

    #[test]
    fn find_agent_file_supports_agents_md_variant() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("Agents.md"), "# Rules\n").unwrap();
        let found = find_agent_file(dir.path()).expect("should find Agents.md");
        assert!(found.ends_with("Agents.md"));
    }

    #[test]
    fn generate_completions_writes_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("completions/patchloom");
        generate_completions("bash", &target).unwrap();
        assert!(target.exists());
        let content = std::fs::read_to_string(&target).unwrap();
        assert!(content.contains("patchloom"));
    }

    #[test]
    fn completion_install_path_returns_some_for_known_shells() {
        // Requires HOME to be set (normal in test environments).
        if std::env::var("HOME").is_ok() {
            assert!(completion_install_path("bash").is_some());
            assert!(completion_install_path("zsh").is_some());
            assert!(completion_install_path("fish").is_some());
            assert!(completion_install_path("elvish").is_none());
        }
    }
}
