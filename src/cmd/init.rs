use crate::cli::global::GlobalFlags;
use crate::exit;
use clap::Args;
use std::path::Path;

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Skip confirmation prompts.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

/// Candidate files where agent instructions may already exist.
const AGENT_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", "PATCHLOOM.md"];

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
        let content = std::fs::read_to_string(&target_path).unwrap_or_default();
        if content.contains("patchloom") {
            status!("{rel_target} already contains patchloom rules, skipping.");
        } else if auto_yes || confirm(&format!("Append patchloom rules to {rel_target}?")) {
            let mut content = content;
            if !content.ends_with('\n') {
                content.push('\n');
            }
            content.push('\n');
            content.push_str(&rules);
            std::fs::write(&target_path, content)?;
            status!("appended patchloom rules to {rel_target}");
        } else {
            status!("skipped {rel_target}");
        }
    } else if auto_yes || confirm(&format!("Create {rel_target}?")) {
        std::fs::write(&target_path, &rules)?;
        status!("created {rel_target}");
    } else {
        status!("skipped {rel_target}");
    }

    // 2. Shell completions hint.
    if !quiet {
        eprintln!();
        if let Some(shell) = detect_shell() {
            let cmd = completion_command(&shell);
            eprintln!("shell completions ({shell}):");
            eprintln!("  {cmd}");
        } else {
            eprintln!("shell completions:");
            eprintln!("  patchloom completions <bash|zsh|fish|elvish>");
        }

        // 3. MCP setup hint.
        eprintln!();
        if cfg!(feature = "mcp") {
            eprintln!("MCP server is available. Add to your agent's config:");
            if cwd.join(".grok").is_dir() || home_file_exists(".grok/config.toml") {
                eprintln!("  Grok: add to ~/.grok/config.toml:");
                eprintln!("    [mcp_servers.patchloom]");
                eprintln!("    command = \"patchloom\"");
                eprintln!("    args = [\"mcp-server\"]");
            }
            if cwd.join(".vscode").is_dir() {
                eprintln!("  VS Code: add to .vscode/settings.json:");
                eprintln!(
                    "    \"mcp.servers\": {{ \"patchloom\": {{ \"command\": \"patchloom\", \"args\": [\"mcp-server\"] }} }}"
                );
            }
        } else {
            eprintln!("MCP server not available (build with --features mcp to enable).");
        }

        eprintln!();
        eprintln!("setup complete.");
    }
    Ok(exit::SUCCESS)
}

fn find_agent_file(cwd: &Path) -> Option<std::path::PathBuf> {
    for name in AGENT_FILES {
        let p = cwd.join(name);
        if p.exists() {
            return Some(p);
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

fn home_file_exists(rel: &str) -> bool {
    if let Some(home) = std::env::var_os("HOME") {
        return Path::new(&home).join(rel).exists();
    }
    false
}

fn atty_stdin() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stdin())
}

fn confirm(prompt: &str) -> bool {
    if !atty_stdin() {
        return false;
    }
    eprint!("{prompt} [Y/n] ");
    let mut buf = String::new();
    if std::io::stdin().read_line(&mut buf).is_err() {
        return false;
    }
    let answer = buf.trim().to_lowercase();
    answer.is_empty() || answer == "y" || answer == "yes"
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
        let found = find_agent_file(dir.path());
        assert!(found.is_some());
        assert!(found.unwrap().ends_with("AGENTS.md"));
    }
}
